# Gazette MCP operation reference

Gazette meets each actor where it works, with the same publication model behind every mode: a person writing in an editor arrives through the language server, an agent helping in Claude Code or Codex through the MCP server and its skill, and a build or integration through the command line and SDKs.

To work on a publication in Claude Code or Codex, open its folder and [install the Gazette plugin](../integrations/agents/README.md). It starts `gazette mcp` and supplies the Gazette skill. Installation and removal are checked on both hosts; real-model publication work and matching page diagnostics across interfaces remain under qualification. The package does not install the executable or select a publication merely by connecting.

Use this reference when writing a client that calls Gazette. Find an operation by the work it does, then use the live `tools/list` schema to build the request. The MCP server and CLI share the same Rust application.

<a id="mcp-interface"></a>

## Connect a local client

<a id="make-a-complete-supplied-input-call"></a>

[Connect a Gazette MCP client](mcp-quickstart.md) covers starting the server, initializing the connection and [running your first workspace check](mcp-quickstart.md#make-a-complete-supplied-input-call). For the author’s walkthrough, see [Work with an agent](https://rawthoughts.in/gazette/work-with-an-agent/).

## Protocol and tools

Gazette supports initialized sessions negotiating MCP `2025-06-18`, `2025-11-25` or `2026-07-28`. It also supports stateless MCP `2026-07-28`, where each request supplies the protocol revision and client capabilities in `params._meta`.

Both forms use the same tools and application. The transport uses `rmcp 3.1.2` with server and standard-stream features. It does not expose resources, prompts, resumable tasks or progress.

An initialized connection can omit per-request protocol metadata after negotiation. A stateless request must use `2026-07-28`. The server rejects an older revision in stateless metadata.

## Find an operation contract

| Capability / integration question | Exact tools and behavior |
| --- | --- |
| Workspace: compile configuration, inspect origins or composition | [Workspace tools](#workspace-tools), [composition](#composition-phases) |
| Authoring: retain buffers and query current source facts | [Authoring tools](#authoring-tools), [handle lifecycle](#held-authoring-lifecycle) |
| Build: resolve, render, stage or inspect disclosure | [Build tools](#build-tools), [rendering](#complete-local-rendering) |
| Preview: plan, start, inspect, poll and stop | [Preview tools](#preview-tools), [preview lifetime](#preview-lifetime) |
| Documentation: check or assemble manuals and generated reference | [Documentation tools](#documentation-tools) |
| Store: inspect existing storage or hold an ordinary writer | [Store tools](#store-tools), [store lifetime](#held-ordinary-store-sessions) |
| Release: prepare, verify and hold a release writer | [Release tools](#release-tools), [release lifetime](#held-release-sessions) |
| Observation: sample destinations or record diagnostic history | [Observation tools](#observation-tools) |
| Recovery: inspect and close interrupted custody | [Recovery tools](#recovery-tools) |
| Extend an interface | [Shared interface architecture](architecture.md#share-behavior-across-interfaces) |

## Discover tools {#discovery-and-acceptance}

Send ordinary `tools/list` if your client needs the complete catalog in one response. This also works for hosts that read only the first list response and offer their own search. The catalog puts `common` operations before `advanced` ones, then orders by capability, intent and name.

If an initialized client only needs a few capabilities, add this to the request's `_meta`:

```json
{
  "io.rtepl.gazette/discovery": {
    "capabilities": ["authoring", "build", "workspace"]
  }
}
```

Use one or more of `authoring`, `build`, `documentation`, `observation`, `preview`, `recovery`, `release`, `store` and `workspace`. An empty list, duplicate or unknown key, or malformed value fails. Omit the extension to get the complete catalog. This uses the standard [MCP tools method](https://modelcontextprotocol.io/specification/2026-07-28/server/tools).

For a filtered list, follow the returned `nextCursor` with the next request's `cursor`. All pages except the last contain eight tools; the last contains at most eight. Result `_meta` gives the normalized capability list and tool count. The cursor remembers those capabilities, so you can omit them on later requests. If you include them, they must agree. A malformed, noncanonical, wrong-version, misaligned or out-of-range cursor fails. The cursor also includes the SHA-256 of the serialized descriptor catalog, so a changed catalog invalidates it. Clients should treat the token as opaque.

Every paginated response sets `ttlMs` to `0` and `cacheScope` to `"private"`, as supported by the MCP 2026-07-28 schema and `rmcp` 3.1.2. Fetch the catalog again across authorization contexts or process restarts. The protocol has no catalog ETag. The cursor hash only determines whether the cursor can be used.

Each tool has a title, input/output schemas, annotations and namespaced metadata. One registry serves listing and dispatch. Schemas describe the serialized SDK result, including explicit tool-failure and output-limit alternatives. They distinguish a required nullable field from an optional field you may omit. Gazette still checks sources, references and permissions when the tool runs. The result includes structured JSON and a readable rendering of it.

Clients can filter their search by `io.rtepl.gazette/capability`, `intent`, `effect`, `prominence` and `closure`. When supplied, relationships point to existing tools that inspect, verify, observe, reconcile, recover or perform a related contrast. A session's terminal owner is the operation that ends it. Discovery leaves tool names, dispatch, request rules and permissions intact. Load the descriptors your client needs. Gazette offers the complete catalog for hosts that need it.

## Bounds and failures

Keep the request, returned handle and any result that identifies the operation. You need these if a process exits before the client receives its reply.

### Request frames and concurrency

The server accepts at most 1 MiB per newline-delimited request by default, excluding the newline. It checks before parsing JSON. A frame over the limit ends input and is not skipped. This limit counts request bytes and does not cap the memory used by protocol buffers.

To send a larger workspace or receive a larger result, start the server with explicit limits:

```sh
gazette mcp --max-frame-bytes 2097152 --max-response-bytes 2097152
```

| Limit | Accepted value |
| --- | --- |
| `--max-frame-bytes` | 4,096 to 16,777,216 bytes |
| `--max-response-bytes` | 1,024 to 16,777,216 bytes of encoded tool result, excluding catalog or protocol envelope |
| SDK TOML defaults | 64 KiB, 10,000 syntax nodes, depth 64 |
| Simultaneous blocking computations | Two per process; an extra request is rejected without a queue |

Invalid budgets stop the transport before it starts. Embedded callers use `TransportLimits::new` with `run_with_limits` or `serve_with_transport_limits`. Ordinary `run` and `serve` use defaults. A larger transport limit leaves SDK source limits, read permissions and parser limits unchanged. Measure the whole request and use a size large enough for the full result.

### Results and uncertain delivery

Read `io.rtepl.gazette/dispatch` on a JSON-RPC error from `tools/call`. Gazette uses `not-dispatched` or `outcome-unknown`.

| What the client receives | How to respond |
| --- | --- |
| JSON-RPC error with `io.rtepl.gazette/dispatch: not-dispatched` | The tool did not run. Correct the request or use the correct tool name. |
| JSON-RPC error with `io.rtepl.gazette/dispatch: outcome-unknown`, or a missing/unrecognized dispatch value | Save the original private request and any returned handle or IDs. Inspect or observe what happened before attempting another effect. |
| Tool result with `isError: true` | The tool ran and returned a failure. Read its structured diagnostic, such as a syntax error, missing reference or absent origin. |

The SDK result is serialized under a 256 KiB default ceiling before the server keeps its JSON value and readable text. The envelope and JSON escaping add overhead. Exceeding that ceiling usually returns `mcp_output_limit`. Gazette never returns an incomplete successful compilation or analysis.

`prepare_sites`, `record_release_observation`, `enroll_local_coordinator` and release-session operations reserve a complete reconciliation response before they run. If encoding fails after the operation, Gazette returns the original details needed to find its result. The three store-creation tools instead check the resolved directory against `options.output` before creation. If the result cannot be sent normally, they keep the original path bytes and the success header/pin or recorded failure. See [Store](store.md). If even that delivery contract fails, the original result remains available separately. Frames, fixed descriptors and small size errors do not count against the SDK-result ceiling.

Cancellation cannot undo a computation or write already in progress. A worker stays occupied through the computation and result encoding.

### Carrier and runtime lifetime

End input to close the service. A clean exit means the service finished and says nothing about whether the client accepted the last response. Transport failures return an error. The launcher keeps errors separate from protocol stdout.

The launcher owns copies of its standard streams and waits up to 100 ms for runtime shutdown. This lets it return after a broken output pipe even if input is still open. A timeout does not establish that a blocking read or computation stopped. When embedding Gazette, pass asynchronous streams to `serve` and manage their lifetime in your application. LSP uses the same SDK authoring state with its own message format. See the [JavaScript SDK](../sdk/js/README.md) for Node support.

## Application handles and lost delivery

An MCP connection starts with no workspace or application session. Save a handle when you open a session and check how that resource reports readiness. A successful start can precede readiness.

| Resource | How to use its handle |
| --- | --- |
| [Authoring session](#held-authoring-lifecycle) | Start or import returns a generation. Use status and generation-checked queries, then `authoring_close_session`. Four handles with an absolute 300-second lifetime. |
| [Preview worker](#preview-lifetime) | Wait for a ready observation. A stop request does not finish the worker immediately. Release its handle after it finishes. Four handles, with no expiry or automatic restart. |
| [Store session](#held-ordinary-store-sessions) | Open returns readiness. Inspect and close that same session. Four handles, with no expiry or eviction. |
| [Release writer](#held-release-sessions) | Open first, then get authorization for the reported generation. The session owns operations and close. Four handles, with no expiry or automatic retry. |
| [Recovery session](#recovery-tools) | Use the separate recovery tools for an interrupted effect. An ordinary store or release handle cannot inspect it as a recovery session. |

After an uncertain result, keep the original request, handle, pins and operation IDs. Follow the tool's inspect, observe, reconcile or recover relation to decide what to check. A transport error does not prove rollback. Never repeat a request automatically to see whether the first one worked.

### Explicit local reads

Supply the roots to read and the permission required by that tool. A session handle grants no extra filesystem access. The operation descriptions below explain what each call reads and what local state it can change.

## Effect metadata

CLI and MCP discovery describe the same operations. Use the capability and intent to find a tool, and its relations to find checks to run before or after it. `closure` is one of `none`, `related`, `terminal-owner` or `self-terminal`. It tells a client whether the tool has a related check or a specific operation that ends its session.

`io.rtepl.gazette/effect` is the broad class. `read` has no listed local transition or network path, `local` can change local state and `external` can contact a remote. Use `io.rtepl.gazette/potentialEffects` to see what the widest form of a request can do:

| Value | What to account for |
| --- | --- |
| `memory-state` | Changes to state held by the process |
| `filesystem-read` | Local reads |
| `filesystem-write` | Local writes |
| `sqlite` | Database or SQLite coordination-file work |
| `process` | Launching a process |
| `git` | Git mutation. Reading Git data alone does not use this value. |
| `listener` | Opening a listener |
| `network` | Remote reads or mutations |

This is the order of values in the array. An empty array means supplied-input or in-memory computation without the listed effects. The registry supplies the array, and MCP carries it unchanged. A listed effect is a possibility under the operation's request rules. It grants no permission.

For example, `assemble_docs` is `local` with filesystem-read, filesystem-write, SQLite and process facets. A caller can assemble in memory, run allowed generators, keep a staged tree or record a preparation. The Gazette tool has no remote provider or network facet. `observe_github_pages_build` has filesystem-read, process and network facets for a remote read. VS Code validates the discovery fields and uses them in its confirmation text.

Find the tool for the relevant `gazette check` mode under Workspace or Build. The CLI command groups several operations, so the MCP catalog exposes their individual tools.

## Operation reference

Use the live `tools/list` schemas for field shapes and defaults. Use the tables below to choose the operation and understand what a call reads, changes or returns. The live catalog lists common tools before advanced tools, then orders by capability, intent and name.

## Workspace tools

| Tool | What to pass | What it returns |
| --- | --- | --- |
| `contract_schema` | `contract`: `theme-manifest`, `template-context`, `workspace`, `authored-content` or `legal-source` | Draft 2020-12 schema from the corresponding Rust type. No file reads or writes. |
| `check_workspace` | Optional workspace file or folder in `workspace`; omit for lookup. Alternatively supply detached TOML in `source`, with optional `limits`. | Native selection checks configured pages and reports the workspace choice and diagnostics. Detached source checks configuration only. |
| `load_workspace` | Absolute `root`, root-relative `workspace`, `allow_native_reads: true` and optional read/parser limits | Resolved workspace, root manifest and include identities. Reads configuration without building or writing. |
| `workspace_field_origin` | `source`, `FieldQuery` in `query`, optional `limits` | Location of the explicit declaration in the source. |
| `workspace_composition_origin` | `source`, `CompositionOriginQuery` in `query`, optional `limits` | Effective workspace or site setting and where it was declared. |
| `inspect_composition` | Complete TOML `source`, `site`, `allow_read: true`, optional `syntax` and `acquisition` limits | Inventories and pins for the Theme and enabled plugin sources. Does not validate manifests. |
| `check_composition` | Same as inspection | Checked manifests and declared inputs. Does not run a provider or compile a resource. |
| `compile_templates` | Same as inspection, optional `templates` limits | Tera compilation report. No rendered files or program handle. |
| `inspect_site` | Complete TOML `source`, `site`, `allow_read: true`, optional `syntax` and `acquisition` limits | All enabled project-documentation placements and their source pins and inventories. |
| `parse_site_documents` | Same as site inspection, optional `corpus` limits | Parsed Markdown, frontmatter and notebooks, plus IDs for opaque assets. |

For example, a declared-field query is `{"kind":"brand","identity":"brand:one","field":["name"]}`. A composition query is `{"site":"site:one","field":{"kind":"theme"},"path":["source"]}`. The native SDK query deserializers remain authoritative. MCP does not maintain a second configuration field catalog. Unknown argument/query fields are rejected.

Configuration and origins use the same retained SDK syntax and resolver. Ranges are half-open UTF-8 byte offsets in the exact supplied string, not UTF-16 editor positions. Defaults receive no invented declaration coordinates. Successful compilation is not content admission, a build or publisher approval.

### Composition phases

`inspect_composition`, `check_composition` and `compile_templates` accept complete workspace TOML `source`, exact `site`, `allow_read: true`, and optional native `syntax`/`acquisition` limits. Unlike publication acquisition, this path captures only the effective Theme and explicitly enabled plugin-package sources. Capability-provider and resource references do not activate or capture a package. Inspection returns their exact pins and inventories; it does not validate their manifests.

For these composition tools and the [private publication tools](#private-publication-checks), `source` also accepts the same explicit workspace selection as a local publication recipe:

```json
{"kind":"local","root":"/absolute/workspace","workspace":"gazette.toml"}
```

This native form reads only the root's declared include set, preserves its document evidence and resolves workspace-relative source roots against that exact root. `allow_read: true` is still required before any read. A string remains detached TOML and never becomes a path. Native `limits` belong to configuration capture; the operation's `syntax` limits still govern its later manifest checks. No caller should flatten included configuration or infer a root from its cwd.

Checking additionally validates every selected manifest and required file from that one capture. As in the CLI, `syntax` bounds apply independently to workspace parsing and to the complete manifest set. Optional capabilities do not turn invalid manifests or missing declared files into success.

Compilation performs the same metadata checks, then invokes the SDK's Tera compiler over every regular file in the selected template subtree. Its optional `templates` argument is the native rendering-limits object; it includes a nested `templates` source-tree limit. Context/output bounds appear in the exact native compilation evidence but do not cause rendering. The returned report binds the source capture, selected subtree, manifest, configuration, compiler-adapter profile and limits. No compiled handle is retained. Neighboring template-like files and capability scripts are not compiled, executed or silently treated as templates. Neither metadata nor compilation proves a capability implementation, policy approval, rendered output or certification. All three tools remain read-only; native failure and response overflow never return a successful partial phase.

### Site-documentation phases

`inspect_site` and `parse_site_documents` accept complete workspace TOML in `source`, exact `site`, explicit `allow_read: true`, and optional native `syntax` and `acquisition` limits. They acquire only enabled project-documentation placements for that site. Disabled and other-site placements are not read; an empty selected placement set is valid, not an inferred fallback source. Complete workspace references are still checked. This differs from authored-publication and Theme/explicit-plugin-package acquisition.

Inspection returns the whole configuration and every selected placement's logical target, exact repository or parent-gitlink evidence and inventory. `parse_site_documents` additionally accepts native `corpus` limits, including independent Markdown, TOML-frontmatter and notebook ceilings. It parses every retained file without reopening sources. Plain Markdown, explicitly framed TOML documents, notebooks and opaque assets retain their existing native inspection shapes and byte coordinate systems; assets remain hashes/counts, not admitted public files. Invalid supported syntax never falls back to an opaque asset.

Both operations require fresh explicit acquisition on each call and return no partial successful report on capture, syntax, projection or carrier exhaustion. They perform no Theme/resource compilation, notebook execution, staging, database operation, network access, public admission or certification. Reports may contain private source text, metadata and native paths. Output schemas derive from the same serialized SDK report owners as the CLI.

### Load workspace files {#load-a-selected-workspace}

Use `check_workspace` to check the publication in an opened folder, or pass `workspace` to select a file or folder. It follows the [shared workspace lookup](workspace.md#configuration-inputs-and-boundaries) and returns the same page diagnostics as `gazette check`. Older hosts can supply their first local folder through `roots/list`; Gazette uses it before the process start folder. Supplying `source` instead retains the detached configuration-only operation.

Use `load_workspace` to inspect configuration from an exact native root and root-relative manifest without checking its page sources. This lower-level loader follows declared non-recursive includes under its read and parser limits. It does not perform workspace lookup.

## Authoring tools

| Tool | What to pass | What it returns |
| --- | --- | --- |
| `inspect_markdown` | One `source` or `tree: {root, identity, document, limits?}`. For a tree, set `allow_native_reads: true`. Optional `front_matter: "toml"`, `html` and `limits`. | Parsed Markdown or framed document, with HTML if requested. The tree result identifies the bytes read. |
| `inspect_source` | Absolute `root`, `allow_native_reads: true`, tagged `selection` and read limits | File inventory, verified Git revision or verified parent gitlink. |
| `inspect_toml` | Absolute `root`, `allow_native_reads: true`, optional confined `subdirectory` and `limits` | Complete TOML inspection. Any failure stops the whole call. |
| `authoring_start` | Named `{id,path,text}` in `workspace` and `documents`. Optional non-root includes in `workspace_documents` use the same shape. Optional `manuals` use `{source,id,path,text}`, `page_inputs` use `{id,path,bytes_base64}` and `package_baselines` use a typed Theme/plugin identity plus a `{id,path,text}` manifest. | A handle, generation and `changed` acknowledgement. Without `workspace_documents` the caller supplied one workspace buffer; otherwise Gazette checks the include graph. Manuals get Markdown analysis. Opaque inputs keep their bytes and cannot be text overlays. Manifests stay tied to their package IDs. The tool opens no local path. |
| `authoring_import_editor_context` | One complete editor export in `context`, or `file: {path, sha256, allow_read: true}` for an absolute local file. Native format `/2` also requires outer `allow_native_reads: true`. | A session after Gazette checks the workspace, roles, contents, hashes, open versions and cursor. A failed import keeps no handle. Query the returned generation and use `authoring_close_session` when done. |
| `authoring_status` | `session` | Current generation, paths, digests, overlay versions and analysis state. No source text. |
| `authoring_open_document` | `session`, `generation`, `document`, `version`, complete `text` | Whether the open overlay was installed. |
| `authoring_change_document` | Same as open | Whether the replacement was accepted at its increased version. |
| `authoring_close_document` | `session`, `generation`, `document` | Whether the session returned to the baseline. |
| `authoring_close_session` | `session`, `generation` | Whether the handle was released. |
| `authoring_diagnostics` | `session`, `generation`, `document` | Findings for that source's role: configuration, authored document or Theme/plugin manifest. Repair review rechecks the same role. It returns no proposal if the package diagnostic has no repair implementation. |
| `authoring_page_package` | `session`, `generation`, `document` | Owning leaf document, input role and companion classifications for this source and digest. Uses the compiler's result without searching files or choosing publication content. |
| `authoring_document_symbols` | `session`, `generation`, `document` | Workspace declarations or Markdown headings with locations in the current file. |
| `authoring_search_symbols` | `session`, `generation`, string `query` | Matching declarations and headings with source and version. An empty query returns all. Stale generation or exceeded limits fail without truncation. |
| `authoring_hover` | `session`, `generation`, `document`, UTF-8 `offset` | Current workspace identity and its occurrence or declaration location, or null. No stale result or help for unrelated fields. |
| `authoring_assistance` | `session`, `generation`, `document`, UTF-8 `offset` | Completion and hover from the compiler's schema for workspace TOML, authored frontmatter or a configured reduced legal-source file. Includes range, scope, digest and version. Does not edit a file or decide publication validity. |
| `authoring_component_assistance` | `session`, `generation`, `document`, UTF-8 `offset` | Current component name or property/value replacement range, schema choices and matching template/schema help. In YAML landings, `hover.landing_children` gives Theme choices and when an unnamed group is valid. Results carry the generation and source digest. |
| `authoring_complete_reference` | `session`, `generation`, `document`, UTF-8 `offset` | Workspace, editorial and authored replacements for a typed field or symbolic link, even when its current value is unresolved. Symbolic choices include verified pages, headings and notes. The result gives the current source, version and original text to replace. No edit, stale graph or free-text search. |
| `authoring_complete_heading` | `session`, `generation`, `document`, UTF-8 `offset` | Headings and URL fragments for a literal inline `](#fragment)` link, even when empty or unresolved. Gives the range, original text, source and version. No guesses from titles, reference links, images or external links. No cross-document completion or file access. |
| `authoring_review_repair` | `session`, `generation`, `document`, current `version` (or null), `source_sha256`, UTF-8 `offset` and one full `authoring_diagnostics` finding | Current schema, reference or heading proposals marked `review_required`. Stale sources or altered diagnostics fail. A current diagnostic without a proposal returns null. The tool does not apply a change, write or run a command. |
| `authoring_origin` | `session`, `generation`, `query`: `FieldQuery` or `CompositionOriginQuery` tagged by `kind: field` or `composition` | Declaring TOML file, version, digest and UTF-8 field range, including an effective declaration in another module. Uses the same SDK call as LSP `gazette/origin` without file reads. |
| `authoring_definition` | `session`, `generation`, `document`, UTF-8 `offset` | Current definition or opaque resource location, or null. Cross-module and publication results carry their source binding. |
| `authoring_references` | Same as definition, plus `include_declaration` | All current reference locations with each target's version and digest. |

### Held authoring lifecycle

Use `authoring_start` to select supplied baselines, or `authoring_import_editor_context` for a complete pinned editor export. Keep the returned handle and generation. Query or edit only that selected state, restore an overlay's baseline with `authoring_close_document`, and release the selection with `authoring_close_session`.

One server retains at most four opaque authoring handles. Each has an absolute 300-second lifetime measured from the start of its acquisition call, not from its last use. Requests never extend that lifetime. Expired entries are reclaimed on a subsequent authoring request or registry drop, not by a background timer. Unknown, expired or closed handles require fresh explicit selection. The stateless MCP carrier revision does not make these application handles stateless.

Each handle uses the SDK defaults: at most 4,096 selected sources including the workspace, 1 MiB total identifier bytes, and 16 MiB retained baseline-plus-overlay text. Logical paths and analysis have their own shared bounds. The default 1 MiB request frame still limits each selection or edit; retained capacity is not a larger single-request allowance. IDs are opaque routing keys, paths are logical inputs, and neither authorizes native reads. No source text is returned by status.

Select article-owned `notes.md` as editable `documents`, not opaque `page_inputs`. Notes participate in body assistance and selected heading/link navigation under their article's identity; they require no independent page frontmatter. Manual buffers retain their separate role: publication-target completion is not proof of Mintlify link compatibility.

#### Generations, edits and query coordinates

All operations on an existing handle except status require the exact current generation. Stale generations fail before mutation. Within that guard, stale editor versions, duplicate opens, or changes to unopened/unselected documents use the same SDK no-op rules as LSP and acknowledge `changed=false` without advancing generation. Accepted edits recompute the complete selection; failed current analysis replaces prior success. Closing an overlay restores the selected baseline, never disk. Symbols and navigation share native availability checks: unavailable current facts are not successful empty or stale results. Query reports bind the generation, document, logical path, source digest and editor version. Locations retain their native precision and UTF-8 byte coordinates, not UTF-16 positions or rename ranges.

Workspace navigation uses the same compiler-owned typed declaration/reference index as LSP, not string matching or a transport-owned TOML parser. It spans the exact selected root/include document set. Display names and other unrelated strings do not acquire targets. Invalid current workspace syntax or semantics refuses its symbols/navigation; unrelated Markdown corpus failure does not erase a valid workspace index. Symbol output has two explicit schema-discovered shapes: `{declaration, location}` or `{heading, location}`. Declaration coordinates denote native TOML scalar constructs, including their source quoting, not complete sections.

#### Capacity and lost acknowledgements

Session capacity is checked before selection retention; creation acknowledgement encoding is checked before registering its handle. Mutation acknowledgement capacity is checked before changing existing state. A busy registry refuses concurrent authoring access rather than queueing it. These checks do not undo a completed operation when output delivery is lost. With a known handle, inspect status before retrying; if acquisition delivery lost the handle, reselect subject to the capacity/lifetime limits. There is no cross-process durable recovery.

### Editor snapshots

The [VS Code integration](vscode.md) imports a generation-pinned [LSP editor snapshot](lsp.md) into the existing authoring APIs. Native discovery uses `gazette-editor-context/2`: saved discovery choices and configuration pins, the complete source manifest, active cursor and only changed open text. With receiving-side `allow_native_reads: true`, the shared SDK reads saved inputs, verifies their baseline hashes, replays unsaved configuration and content, and checks complete membership, roles, current hashes, clean open versions and opaque file observations. Changed saved inputs refuse the import even if an overlay could hide that change. Opaque observations retain paths and file lengths; they neither read payloads nor assert content identity. File reads are bounded and not an atomic filesystem snapshot.

The native-read permission belongs beside `context` or `file` in the tool arguments, never inside the export. A local file carrier additionally needs its own `file.allow_read` and independently retained SHA-256; permitting that one export file does not permit workspace reconstruction. CLI verification uses `gazette import-editor-context --allow-read < editor-context.json`. An export cannot authorize its own reads. The self-contained `gazette-editor-context/1` remains supported for explicitly supplied buffers and captured opaque inputs, without workspace reads; CLI verification of that format needs no read flag.

The extension transfers at most 12 MiB of changed text for native discovery; saved sources remain subject to the receiving SDK's separate source and retention limits. The complete request must also fit its transport frame. Explicit supplied-buffer exports retain the 12 MiB complete-source ceiling. No path returns a truncated successful selection. The extension's explicit inspection command grants native reads for its selected snapshot, closes the temporary MCP handle after its query and discards results if the editor generation changed. Other MCP clients do not silently inherit a running editor's unsaved buffers; they must supply their selected inputs explicitly.

### Source and configuration trees

`inspect_source` requires `selection.kind` to be `observation`, `revision` or `submodule`. Observation optionally selects a confined relative `subdirectory`; revision requires that directory and an exact `commit`; submodule requires `parent_commit`, `submodule_path` and the child-relative `subdirectory`. Git coordinates are full lowercase SHA-1 commit IDs, never branches or prefixes. Native selectors reject traversal, dot components, ambiguous escapes and unsafe links. Root selection for unpinned observation is expressed by omitting its subdirectory, not by inserting `.`. Nothing fetches a missing child or substitutes a sibling repository.

Both tools require explicit absolute roots and `allow_native_reads: true` before dispatch. This opt-in does not override host permissions. Source/TOML and Git metadata bounds are projected from their native typed owners; omitted values use native defaults. Each call observes current selected bytes anew. Neither an unpinned observation nor an ordinary filesystem capture claims an atomic snapshot.

TOML inspection captures every selected regular file, including hidden files and files without a `.toml` suffix. Any source or syntax failure rejects the complete set. Choose a dedicated configuration tree. Results can contain private paths, hashes and configuration values; local-read tools advertise `openWorldHint: true` and read-only/nondestructive effects. Result exhaustion fails explicitly rather than truncating an inventory or returning a valid-looking subset.

### Supplied or pinned Markdown

`inspect_markdown` selects exactly one supplied `source` or pinned `tree`. Supplied text performs no path lookup. Tree mode requires explicit native-read permission, an absolute root, an independently retained complete-tree identity and a literal regular-file `document`. The shared [SDK input owner](../sdk/rust/src/markdown/input.rs) captures every entry, including hidden and unrelated assets, verifies the pin, then parses retained bytes without reopening the document or falling back to text. Tree entry/depth/file/total-byte limits remain separate from source/node/depth/fact/output and TOML metadata limits. Changed assets, missing/non-file documents, invalid UTF-8 and unsafe trees fail without partial evidence. The optional `input` envelope records the verified identity and selected document. Optional HTML must succeed completely. Framed-document body ranges remain relative to the reported body span. This is syntax/rendering evidence, not authored admission, Git lineage or perpetual immutability. The tool remains read-only/nondestructive and advertises `openWorldHint: true` because its explicitly selected tree mode accesses native files.

## Build tools

| Tool | What to pass | What it returns |
| --- | --- | --- |
| `check_stage` | Authored and parent roots, read permission, all placements and separate preparation/composition limits | Checked inventory without a stage or a write lease. |
| `stage` | Same as stage check, plus `create_stage: true`, required `directory_bytes` (1..=1048576) and optional `name` | Complete staged inventory and directory, or a failure with the original identity and any remaining paths. |
| `probe_output` | Absolute `root`, `allow_native_reads: true`, expected tree in `expected_tree`, private `rules_json` and separate request/read/scanner limits | Disclosure report over the files with coverage and findings. No file changes or approval. |
| `inspect_publication` | Complete TOML `source`, acquisition `site`, `allow_read: true`, optional `syntax`, `acquisition`, `corpus` and `graph` limits | Source capture, composition and complete graph for private inspection. |
| `review_publication` | Same as publication inspection, optional `review` limits and source-bound `retained_math` results | Private review, nested uses and digest. Does not run a typesetter or checker or approve output. |
| `project_registry` | Same as publication inspection, optional `registry` limits | Registry catalogs, graph, routes and their source information. Does not touch the database. |
| `materialize_sites` | Ordinary writer `session`, `publication`, distinct per-site `ids`, independent `allow_write` and `allow_record`, set-wide `limits` and `evidence_bytes` | Write results or the original render, plan or write error. If delivery fails, keeps the identifiers needed to inspect the attempt. The writer stays open for inspection or close. No retry or release. |
| `render_sites` | `recipe_json` or native `workspace`, `local: true`, `allow_read: true`, optional math switches and syntax/read/per-site and aggregate render limits | Complete source and site-output report, including failed sites. No output directory or publication approval. |
| `qualify_sites` | `recipe_json` with the embedded SDK schema, `allow_read`, `allow_policy_execution`, `output` limits and request/result ceilings | Technical check report. Sets `isError` unless all sites pass. A reserved response warns if checkers or generators may have run before delivery failed. No retry or release preparation. |
| `stage_rendered_site` | One site in `publication`, plus `create_stage`, absolute `output_parent` and required `directory_bytes` | Original render/source report and fresh output directory and inventory, or stage failure with the paths needed for inspection. |
| `probe_rendered_site` | One site in `publication`, private `rules_json`, optional `max_rules_bytes` and `scan_limits` | Disclosure findings and coverage using the original media declarations. No stage, second read or publication. |

### Inspect the publication before rendering {#private-publication-checks}

`inspect_publication`, `review_publication` and `project_registry` accept complete workspace TOML in `source`, an exact configured acquisition `site` and independent `allow_read: true` permission. All three use the same SDK capture, corpus parser and semantic resolution as `check --publication`. Native `syntax`, `acquisition`, `corpus` and `graph` bounds remain separate; nested document and notebook limits are discoverable. Review additionally accepts `review` bounds; registry projection accepts `registry` bounds. Unknown or positional options are rejected before reads.

For a mathematical corpus, pass the complete `math.results` object from an explicitly authorized native render as `retained_math`. It is a JSON object, not a JSON string or a filename. The shared command admits it through `ParsedPublication::with_math`: paths, body identities, syntax profiles, locations and formula occurrences must match the current captured corpus. Missing, extra or stale results fail; the ordinary formatter still validates emitted MathML. Omitting required results returns `publication_review_math_required` instead of an incomplete formatting-plan error. Review never runs a typesetter. Supplied results do not attest their producer or grant approval. Their complete encoded input is bounded independently by `review.bytes`, alongside the protocol's frame limit. The [CLI review route](render-inspection.md#review-exact-retained-surfaces) accepts this same result object on standard input.

Inspection returns the original source evidence, composition and complete graph. Its `authoring_sources` retain the compiler's exact source key, captured entry, logical path and digest for each supported text input, including composed legal inputs. Editor launchers consume these coordinates instead of reconstructing private legal paths or treating every Markdown file as a publication page. The separate `authoring_inputs` list identifies opaque files needed by those text semantics, such as declared listing artwork. It does not select the entire media library for editor retention or turn those bytes into editable documents. Review generates the native private surfaces and their nested uses, including Markdown/notebook/raster projection, without Theme compilation, resource programs or a policy process. Registry projection retains all configured catalogs, including empty-content cases, routes and exact content provenance. The acquisition site does not turn the complete registry into a site-filtered replacement set.

These tools retain drafts, foreign references and private configuration exactly as their CLI/SDK owners do. They neither filter for public admission nor open a database, create staged output or authorize publication. Native failures and complete-result limits never return a successful partial projection; carrier overflow is an explicit output-limit failure. Fresh local source observations may differ between calls, but each call resolves from its one retained capture.

### Complete local rendering

`render_sites` accepts exactly one of `workspace` (the tagged local or supplied workspace input) and `recipe_json`, the same complete [`gazette-local-publication-set/1` request](local-rendering.md#local-site-sets) used by the CLI and preview, plus `local: true` and `allow_read: true`. Its `recipe_json.contentSchema` is generated from the SDK request owner. The exact JSON string preserves duplicate keys for native rejection. There is no second MCP selection parser, schema catalogue or renderer. The invocation captures the current UTC presentation year once, as the CLI does; an explicit `render_year` in the recipe takes precedence. No template reads a clock.

The optional `corpus` object selects the shared authored-corpus limits, including opaque-asset bytes. It reaches the same parser used by CLI `--max-corpus-bytes` and also applies when this request is nested in a preview. Acquisition, per-site output, aggregate output and MCP response ceilings remain independent. For `source_bytes`, an explicit invocation value wins over `[build].corpus_bytes`, then the native 16 MiB default. A partial object such as `{"files":100}` still inherits the configured byte limit. Other parser, acquisition and output bounds retain their independent defaults.

`[build].math` supplies the default math selection. The call or recipe may override it with `math`; the call's `allow_execute` grants math execution permission independently of configuration. An explicit recipe `math_generation` also selects math and uses shared defaults for omitted limits. `diagram_generation` retains its own permission and bounds before the configured DOT renderer can run. The programs are not sandboxed. Consequently `render_sites` advertises `readOnlyHint: false`, `destructiveHint: true` and `idempotentHint: false`; annotations grant no permission. Omitted math selection follows the workspace, then defaults off; omitted diagram selection stays off. Pure rendering consumes the exact retained results. Failure or cancellation is not rollback or permission to retry.

The tool acquires the explicitly selected pinned sources once and renders the whole selected set through the shared model, resource compilers and Theme. A one-site request is the same workspace operation. The report is the native `PublicationSetInspection`: source observations, complete candidate intent and every site's output inventory/evidence or failure. Any failed site sets `isError: true` while preserving successful sibling evidence. A shared prerequisite failure returns its native failure class, not a fabricated successful subset. Text and structured results contain the same private data; neither belongs in public site output.

`syntax`, `acquisition`, per-site `output`, `max_total_output_bytes` and `max_total_output_entries` expose their independent native bounds. The complete recipe additionally obeys `max_input_bytes` and the server frame ceiling. The result obeys the server response ceiling; overflow delivers an explicit error rather than a successful prefix. Native limits can reject a request even when its transport fits. Source-read permission does not grant policy-process, network, staging, database, preview-listener, signing or publication effects.

Generated files live only in memory for this operation; the returned inventory is not a retained output directory or handle. Use `stage_rendered_site` for fresh private retention or an explicitly selected preparation operation for its durable workflow. Unfiltered requests remain draft-inclusive; an explicit selection is a non-draft candidate, never public admission or certification.

`stage_rendered_site` and `probe_rendered_site` nest the same arguments under `publication`, require exactly one explicitly selected site before source acquisition, and consume the original retained native render. They do not introduce another renderer or scanner. Staging requires independent `create_stage: true`, an absolute `output_parent` and `directory_bytes` in `1..=1048576`; the native writer checks the actual canonical path and protects all consumed roots before creation. Its complete report carries native inputs, render evidence, inventory and directory. The same source-staging delivery owner reserves response capacity before effects: overflow returns `stage_receipt_delivery` with the original inventory identity and lossless directory bytes; a retention failure preserves `unverified_residue`, not a successful artifact. No cleanup, replacement or retry follows delivery failure.

Producer probing accepts exact private `rules_json` in the existing `gazette-disclosure-output-input/1` format, `max_rules_bytes` and the same scanner overrides as [`probe_output`](#pinned-imported-output-disclosure). Generated media declarations select syntax even when filenames have misleading suffixes. No native tree is recaptured, staged or rewritten. The native result remains partial evidence, including zero findings; it neither admits publication nor certifies output.

### Fresh source staging

`stage` and `check_stage` share one strict input projection and the SDK's source acquisition/composition owner. Both require `allow_native_reads: true`, absolute `authored` and `parent` paths, explicit `workspace`, `site`, complete `placements` (including `[]` for authored-only), and independent `preparation`/`limits` objects. Project inputs retain exact configured revision bindings. Authored bytes are captured during this operation; they are not independently revision-pinned.

Only `stage` accepts `create_stage: true`, optional `name`, and required `directory_bytes` (1..=1048576). The native writer bounds the **actual canonical directory**, including ancestor aliases, before creation. A selected name gets one creation attempt; collisions preserve the existing object without fallback. The parent must be an existing private namespace disjoint from protected inputs. The operation grants no content admission, artifact certification or release.

Complete success has the CLI shape `{status, workspace, site, inventory, directory}` with `status: "retained"`. Response capacity is reserved before acquisition or effects. If the full receipt cannot fit, the tool returns an error with `stage_receipt_delivery`, exact `directory_unix_bytes` and the **original** `inventory_identity` under retained evidence. It neither reads back a replacement pin nor removes output to hide failed delivery. Native failures distinguish `no_stage` from `unverified_residue`; neither grants cleanup or retry authority. Diagnostics may be omitted from compact evidence, never the locator or retained identity. A disconnected caller must reconcile native effects rather than infer that nothing happened.

### Pinned imported-output disclosure

`probe_output` projects CLI `probe-output`, not the separate rendered-site probe. Supply `allow_native_reads: true`, an absolute `root`, independently retained `expected_tree: {format, sha256}`, and private `rules_json` using the existing `gazette-disclosure-output-input/1` envelope with unique named `id`/`literal` rules. CLI and MCP use the same SDK decoder; malformed/duplicate/positional input is rejected without echoing private values. The embedded discovery schema describes that native envelope; the scanner still owns rule validation.

`max_rules_bytes` bounds complete private JSON before parsing or native reads. `source_limits` independently bounds the exact pinned tree capture. `scan_limits` exposes the shared SDK/CLI scanner overrides: `encoded_input_bytes`, `decoded_bytes`, `encoded_depth`, `encoded_values`, `encoded_bytes`, `work_bytes` and `html_tokens`. HTML callbacks are bounded per input, including embedded Markdown HTML; the default remains 100,000. Zero is an explicitly exhausted budget, not an instruction to use the default. A larger selected budget changes the report's resource digest without changing disclosure-policy admission. Omitted values and unexposed scanner limits retain native defaults; this is not a second comprehensive scanner configuration. MCP response encoding has its own ceiling and cannot return a truncated successful report.

The native scanner receives no invented producer media map. Imported files retain the existing suffix-selected decoding coverage; unknown formats may remain raw-only. The complete native report includes tree/rule/evidence digests, coverage and findings, and always reports `partial-evidence`, including when no literal matches. It contains no separate literal or payload snippet, but paths and rule identifiers can themselves be private. No rendering, staging, rewriting, browser execution, policy admission, certification or release follows from this diagnostic operation.

## Preview tools

| Tool | What to pass | What it returns |
| --- | --- | --- |
| `plan_preview` | Full `start_preview` request | Plan showing what the request would do. Reads no sources and starts no listener, process, network, Git or release operation. |
| `start_preview` | Shared `publication`, `listen` site/address pairs, independent `allow_listeners`, pinned/development `mode`, optional local URLs and transport/history limits | Worker handle. Wait for a ready event before using its URLs. |
| `plan_production_preview` | Shared `publication`, `listen`, `registry: "unconfigured"`, `artifact: "fresh_render"`, listener permission and limits | Plan for a fresh pinned preview with public links. Does not start it. |
| `start_production_preview` | Same production request, optional history limits | Worker handle. Does not write the registry, write an output directory, push or prepare a release. |
| `inspect_preview`, `poll_preview` | Preview session; for `poll_preview` add `after` and `maximum` | Snapshot or a limited page of events without consuming them. Reports any overwritten prefix. |
| `stop_preview` | Session, optional `release_handle` | Requests stop, or removes the handle once the worker has finished. Stop request alone does not prove shutdown. |

### Preview lifetime

`plan_preview` and `start_preview` nest the same `render_sites` arguments under `publication`, including `recipe_json`, `local: true`, `allow_read: true` and selected limits. It additionally requires a complete `listen` array of `{site, address}` objects, independent `allow_listeners: true`, and an explicit `mode`: `{"kind":"pinned"}` or `{"kind":"development","interval_ms":500}`. Optional `local_urls: true` selects held loopback URLs. Native transport and `history` bounds never enlarge MCP response capacity.

The returned session and original start-request digest identify an admitted worker, not listener readiness. Inspect or poll until its native `ready` event identifies every actual socket, local entry URL and source/output identity. Serving proceeds without MCP polling. Pinned mode retains exact captured bytes; development mode uses the same fixed-binding rebuild loop as the CLI.

`inspect_preview` takes the exact `session`. `poll_preview` also requires `after` (zero initially) and `maximum` (at most the selected history event count). Polling is non-consuming: repeat a cursor after lost delivery. An overwritten prefix produces an explicit `gap`; latest generation/stale state and terminal failure remain independently available. No poll causes a build or source change.

`stop_preview` requests closure at native checkpoints. It does not interrupt in-progress capture, compilation or HTTP work. `stop_requested` is not `finished`, and a `closed` event precedes final worker completion. `release_handle: true` removes only an actually completed worker; an active worker retains its handle. Completed handles count toward the four-handle ceiling until explicitly released. There is no expiry, eviction or automatic restart.

Compact handle/digest delivery capacity is checked before effects. Full-result overflow returns an explicit error preserving custody, not a successful prefix. Recover using that handle; never retry start automatically. Service teardown requests stop, but individual request cancellation or response loss does not establish closure. Process termination releases OS sockets without proving a close receipt was received.

`plan_production_preview` and `start_production_preview` deliberately accept a smaller closed request than ordinary preview: `registry` must be `unconfigured`, `artifact` must be `fresh_render`, and no development mode or local URL mapping exists. They reuse the same publication request, loopback listener records, transport bounds, custody and lifecycle tools. This is production-linked local inspection, not certification, release preparation, push or deployment.

Preview serves output from memory without staging a publication or writing the journal. Its publication recipe may separately permit native math or diagram programs before rendering; those processes have their own effects and are not sandboxed by source-read or listener permissions. Inspect a failed or interrupted invocation before retrying. The [native contract](preview.md) defines these permissions, URL modes, stale-generation behavior and browser network boundary.

## Documentation tools

| Tool | What to pass | What it returns |
| --- | --- | --- |
| `check_documentation` | Complete workspace TOML in `source`, `site`, optional syntax limits | Documentation destination and resolved manual/reference plan. Does not read sources, run a generator, write, use Git or contact a remote. |
| `assemble_docs` | Supplied or local `workspace`, `site`, `local`, `allow_read`, required `directory_bytes`. `generation`, `output_parent` or `preparation` need `allow_stage`; generation also needs `allow_execute`. | Manual/artifact and companion identities, generator invocations and staged paths. If delivery fails, keeps the original stage details. Does not replace the destination or publish it. |
| `verify_generated_tree` | Absolute `root`, `allow_native_reads`, `expected_binding`, independent `manifest_sha256`, optional source `limits` | Verified manifest and whole inventory without the marker in the inventory. |
| `materialize_docs` | Ordinary writer `session`, documentation request in `documentation`, operation `id`, independent `allow_write` and `allow_record`, optional protected roots and limits | Assembly and write outcomes separately, or the original documentation and writer details after failure. Inspect or close the writer explicitly. Does not verify hosting or release. |

### Authored-manual and generated-reference documentation

Choose the input mode for the operation:

- `check_documentation` accepts complete supplied workspace TOML in `source` and the exact configured `site`.
- `assemble_docs` accepts the shared supplied/local `WorkspaceInput` in `workspace` and that exact `site`. For detached bytes use `{ "kind": "supplied", "source": "<complete workspace TOML>" }`.
- For a native modular workspace use `{ "kind": "local", "root": "/absolute/workspace", "workspace": "gazette.toml" }`. This retains the selected owner and declared include set. Relative configured destinations require that native owner; they never resolve against the MCP process directory.

Brand configuration owns repository names/paths, project membership, manual selections, generators, formats and ordered navigation. Neither tool discovers projects or reconstructs `domain_doc`. Ordinary authored project documentation uses the separate Gazette Theme-rendered publication path.

`check_documentation` parses only supplied bytes and returns the same resolved target plan used by assembly. It performs no native reads and accepts none of the assembly effect permissions. Use it for target-aware agent/editor discovery before requesting the effectful `assemble_docs` operation.

`local: true` and `allow_read: true` permit configured source acquisition. Supplying `generation` or `output_parent` additionally requires `allow_stage: true`. `generation.allow_execute: true` independently permits the configured commands under the [trusted-code model](architecture.md#trusted-code-and-saved-results); they are not sandboxed, and a zero exit or version string is not generator qualification. Missing generation selection never silently substitutes imported bytes.

For reciprocal local links to product help, the optional `preview_link_bases` object selects exact public-to-loopback bases. It is a private preview input, not a workspace setting; durable preparation and maintained-output replacement refuse it. See [local preview links](mintlify.md#local-preview-links) for the shared map and path-preservation rules.

Both parent paths must be absolute existing native directories. `directory_bytes` is required at the request root and bounds every canonical retained path before creation. Within `generation`, `max_jobs` and `entry_bytes` are explicit, not hidden defaults. For a small two-job request, this selection fits the normal 256 KiB result allowance:

```json
{
  "workspace": {
    "kind": "supplied",
    "source": "<complete configured workspace TOML>"
  },
  "site": "site:one",
  "local": true,
  "allow_read": true,
  "allow_stage": true,
  "directory_bytes": 1024,
  "output_parent": "/absolute/private/target-parent",
  "generation": {
    "parent": "/absolute/private/job-parent",
    "max_jobs": 2,
    "entry_bytes": 1024,
    "allow_execute": true
  }
}
```

Replace those example values with the actual configured site, workspace selection and existing private directories. `generation.timeout_ms` defaults to 30,000 and `max_capture_bytes` to 1 MiB, matching the CLI's native lowering. Optional `syntax`, `acquisition`, `generation.input` and `output` expose native bounds. Whole-generation input/output budgets are enforced by the application, not independently enlarged per module. None of these options increases the MCP server's frame/result ceiling.

The same SDK report and output schema feed CLI and MCP. The SDK reserves complete custody delivery before generation or retention. An insufficient response reservation refuses before effects; a full report that does not fit yields an error with exact compact custody, never a partial success or a generic instruction to retry. Compact custody keeps version-output digests instead of copying potentially large version text. Later-job failures retain completed job pins, active invocation evidence and every known retained native locator as exact Unix bytes. Text and structured MCP content carry the same JSON. Broken delivery or cancellation does not undo native work.

Native generation produces hosting-independent retained artifacts; the existing Mintlify adapter alone owns its accepted target conversion and assembly. Unsupported Rustdoc formats remain explicit failures with native generation custody preserved. Successful local assembly or staging is not a GitHub push, a Mintlify build, a served deployment, public admission or release authorization.

For an existing configured output directory, `materialize_docs` takes that same input under `documentation`, an already-held ordinary writer `session`, an exact operation `id`, and independent `allow_write` and `allow_record` permissions. Fresh `output_parent` and `preparation` retention are incompatible with this route. The native sequence runs once; its original documentation JSON and generator custody remain beside the separate output outcome. The session stays held for explicit `inspect_store_session` and `close_store_session`. Follow [the configured documentation output procedure](output-materialization.md#documentation-uses-the-same-writer-not-the-theme-renderer) for selection, permissions, read-back and lost-response handling; assembly alone never writes that destination.

### Verify generated documentation already on disk {#verify-retained-generated-documentation}

`verify_generated_tree` requires `allow_native_reads: true`, an explicit absolute `root`, independently expected binding and manifest identity. It never creates, repairs, retains, fetches, signs, synchronizes or publishes an artifact. Do not derive an expected identity from the suspect bytes being checked.

`verify_generated_tree` uses the same [generated-tree verifier](../sdk/rust/src/lib.rs) and borrowed manifest/inventory projection as the CLI. A matching generated marker is not enough: all retained documentation entries must match its pinned tree. Its source lineage does not establish current source reachability, rights or publication admission.

## Store tools

| Tool | What to pass | What it returns |
| --- | --- | --- |
| `open_store_session` | Named reader or writer `request`, permissions and expected writer header when applicable | Ready ordinary store session and process-local handle. Does not open a release session. |
| `inspect_store_session` | `session` | Current state from that same reader or writer without reopening it. |
| `close_store_session` | `session` | Close outcome. Does not reset generation or settle release. |
| `reconcile_registry` | Publication inputs, absolute journal `root`, `expected_header`, `options` with separate replacement and writer permissions | Complete replacement result or its projection, separation, open, write or close failure. No partial replacement or retry. |
| `inspect_store` | Absolute database `path`, `allow_read: true`, `read` with `allow_coordination_files: true` | SQLite metadata and header. To check Gazette schema and integrity, use `inspect_journal`. |
| `inspect_journal` | Same as store inspection, optional `expected_header` | Verified schema, integrity and journal coordinates. Compares the entire header when supplied. |
| `inspect_registry` | Same as journal inspection | Verified journal and recomputed registry head, when present. Does not return rows or reconcile. |
| `inspect_materialization` | Absolute database path, historical absolute `output_root`, optional separately saved header and read/coordination permissions | Verified history for the requested output root. Does not read its current files, acquire a writer or authorize a retry. |
| `initialize_store` | Empty private absolute `directory`, `options` with creation/reservation/coordination permissions | New journal result, or the original locator and failure details. Does not enroll a coordinator. |
| `backup_store` | Absolute `source` and private `parent`, `allow_read: true`, `options` with creation/coordination permissions | New backup and manifest pin, or the original locator and failure details. |
| `verify_backup` | Absolute backup `directory`, separately saved `manifest_sha256`, `allow_read: true`, verification `options` | Backup check against the pin. Does not create or repair a file. |
| `prepare_store_restore` | Absolute `backup` and private `parent`, original `manifest_sha256`, `allow_read: true`, `options` with preparation/coordination permissions | Separate restore copy and original preparation pin, or original locator and failure details. Does not activate the copy. |
| `adopt_store_restore` | Absolute anchor, old journal, detached restore and new successor roots. Separately saved binding, clean seal and restore pin. Read/adoption/anchor permissions. | Successor seal at exactly one newer fence. Replaying the same request returns the recorded transition. Stale, dirty or mismatched inputs stop it. |
| `enroll_local_coordinator` | Absolute `anchor_root`, `journal_root`, `coordinator_id`, `principal_scope`, `protected_roots`, independent read/anchor-write/coordination permissions | Fresh seal. If the result will not fit, returns the original input details without guessing a binding or adopting a journal. |
| `inspect_local_coordinator` | Absolute `anchor_root`, separately saved `binding` and read/coordination permissions | Recorded clean or in-progress state. Does not check or repair the current journal. |
| `inspect_release_authorization`, `inspect_release_leases`, `inspect_release_consumption`, `inspect_release_attempts`, `inspect_release_reconciliations` | Absolute database-file `path`, exact `record_id`, `allow_read`, native `read`; optional `expected_header` | Native immutable authorization, lease, consumed intent, attempt or complete reconciliation history; original coordinates remain distinct from the current journal observation |

### Held ordinary store sessions

`open_store_session` takes a named `request` with `kind: "connection"` or `kind: "writer"`. Connection selection supplies an absolute database `path`, absolute `coordination_root`, `allow_native_reads: true` and native `SessionOptions` in `options`. Writer selection supplies an absolute journal `root`, the complete independently retained `expected_header`, `allow_native_reads: true` and native `WriterOptions`. Read, local reservation, coordination-file and generation permissions remain separate. Nothing initializes or migrates a store, accepts a replacement expected header or acquires a release coordinator.

Successful open returns actual native readiness and a process-local `session`. `inspect_store_session` and `close_store_session` take that handle. Inspection reads the same held connection or writer again, never cached readiness or a new generation. A failed native read invalidates the lifetime; restoring a path does not heal it. Close consumes the handle even if native close fails. Four handles are allowed, with nonblocking serialized operations, no expiry and no eviction. Server termination drops resources without successful-close evidence.

Response capacity is reserved before acquisition or another native operation. An acquisition failure retains the original generation-effect classification; failed readiness is distinguished from acquisition refusal and reports its separate close failure, if any. Successful-open overflow retains the active handle. `active: false` identifies a correlation token with no retained native lifetime; it must not be used as proof of rollback. Never automatically reopen after lost output. These handles cannot be used with release-session tools and grant neither cross-host fencing nor publisher authority.

### Existing storage

`inspect_store`, `inspect_journal` and `inspect_registry` call the same one-shot [SDK owners](../sdk/rust/src/store.rs) as their CLI counterparts. Supply an explicit absolute database-file `path`, not a journal directory. No current workspace, database path or expected coordinate is inferred. `allow_read` and the native `read.allow_coordination_files` are independent permissions and both default to false. Every native `ReadOptions` ceiling is exposed unchanged; `busy_timeout_ms` is a per-lock retry bound, not a total operation deadline.

Store observation reports raw store-adapter/header metadata only. Journal inspection additionally checks supported schema and integrity. Registry inspection verifies the complete stored row identity, returning a registry head or `null` for a genuinely empty initialized registry. It does not return row contents or replace them. The latter two accept `expected_header`: a supplied object is compared in full; omission or `null` observes without that comparison. Neither matching nor newly observed coordinates establishes freshness against copies/restores, ownership, a held writer fence, admission or release authority.

These tools use SQL-read-only connections but can maintain WAL/shared-memory coordination files. Accordingly `readOnlyHint` is false, `destructiveHint` is false, and `idempotentHint`/`openWorldHint` are true. They do not initialize, migrate, repair, acquire sessions, advance generations, reconcile registries or contact a network. Use trusted private paths. Native refusals and response overflow return complete failures, not metadata prefixes; cancellation or lost delivery cannot prove that no coordination-file effect occurred. Native schemas describe each actual SDK result and shared bounded failures.

### Trusted-local coordinator discovery

`enroll_local_coordinator` and `inspect_local_coordinator` project the native [coordinator guide](store.md), not a second control store. Enrollment takes exact `anchor_root`, `journal_root`, textual `coordinator_id`, `principal_scope` digest and `protected_roots`; inspection takes `anchor_root` and the independently retained `binding`. Both require `allow_read`, `reserve_local_coordination` and `read.allow_coordination_files`. Enrollment also requires `write_coordinator_anchor`. No permission is inferred from another flag. Discovery describes complete native seal/state outputs, bounded failure/delivery outcomes and coordination-file effects. An enrollment fallback must fit before native access; response loss never authorizes reenrollment or reset. Neither tool is release activation, generation acquisition, signing or repair.

## Release tools

| Tool | What to pass | What it returns |
| --- | --- | --- |
| `verify_release_authorization` | Original strings `authorization_json`, `binding_json`, optional `limits` | Signature and binding digests, or a stable error without private input bytes. |
| `open_release_session` | Complete SDK `OpenRequest` in `request`, including coordinator, expected header and permissions | Ready writer generation and handle, or open failure with the details needed for inspection. |
| `operate_release_session` | `session`, tagged SDK `operation` | Inspection, registration, consumption, attempt, reconciliation or close result. Uses the same writer generation and application rules. |
| `close_release_session` | `session` | Close outcome. Removes the slot even if close fails. Does not settle the release. |
| `inspect_release_checkpoint` | Absolute database `path`, `checkpoint_id`, `allow_read`, `read`, optional `expected_header` | Recorded checkpoint or absence. Does not check current Git objects. |
| `checkpoint_release_commit` | Absolute journal-directory `root` and independent `repository`, `checkpoint_id`, `commit`, `expected_tree`, original `metadata_json`, `expected_header`, `protected_roots`, separate read/generation/reservation/coordination/`record_checkpoint` permissions and limits | Checkpoint, or failure with its progress. Reserves the original IDs and locations before writes in case the response is too large. |
| `recover_journal_worktree` | Absolute journal `database`, `preparation_id`, `commit`, `expected_header`, `protected_roots`, separate read/generation/reservation/coordination/`record_preparation`/`synchronize_worktree` permissions and limits | Recovery result or failure with its progress. Uses only the recipe in the journal. Does not recreate a worktree or retry. |
| `inspect_journal_worktree` | Absolute database `path`, `preparation_id`, `commit`, `protected_roots`, `allow_read`, `read`, `worktree_limits`, optional `expected_header` | Journal, Git and whole output inspection with state checks around the operation. Does not repair or infer completion. |
| `inspect_release_preparations` | Absolute `database`, required `expected_header`, ordered `selections_json`, `protected_roots`, `allow_read`, `options` | All private target records and their original identities in request order. Fails the whole set if any cannot be checked. |
| `verify_prepared_authorization` | Same as preparation inspection plus original `authorization_json` and `binding_json`, optional signature `limits` | Digests from matching signatures to the preparations. Supplying a binding does not obtain a new approval. |
| `verify_release_commit` | Absolute bare repository `root`, `allow_native_reads`, `commit`, `expected_tree`, `metadata_json`, optional `max_metadata_bytes` and revision `limits` | Complete commit-object check. Does not verify the current worktree. |
| `prepare_release_commit` | Absolute bare repository `root`, `output_root`, absolute `protected_roots` array, `allow_native_reads: true`, `allow_write_objects: true`, `expected_tree`, `metadata_json`, optional `limits` and `max_metadata_bytes` | Detached commit or the recorded write failure. Reserves response space before writes. Does not change refs or the journal. |
| `retain_release_commit` | Absolute bare repository `root`, absolute `protected_roots` array, `allow_native_reads: true`, `allow_retain: true`, `commit`, `custody_id`, `expected_tree`, `metadata_json`, optional `limits`, `storage_bytes` and `max_metadata_bytes` | Report on the commit held under its recovery ref, or the original effect/failure. Does not prepare a release, change a branch or access the journal or remote. |
| `inspect_release_custody` | Same commit expectations plus `custody_id` | Report on the recovery ref and commit object without synchronizing or writing it. |
| `prepare_sites` | `recipe_json` with embedded schema and nullable `coordinator`, independent permissions including `write_coordinator_anchor`, `read` and `output` limits and request/result ceilings | Complete preparation result or the phase that failed. Preserves the journal, header, preparation and coordinator IDs if the full result cannot be sent. |

### Local preparation

The [local preparation contract](releasing.md) describes `prepare_sites` permissions, shape, native effects and interruption behavior. Its `readOnlyHint` and `idempotentHint` are false; `openWorldHint` and `destructiveHint` are true. Local compilation does not contact remotes, but the separately permitted native checker is not sandboxed. Retain the original request coordinates before invoking this effectful operation.

Selecting `coordinator: { "anchor_root": ..., "binding": ... }` in the recipe uses the existing trusted-local guarded writer for every native preparation phase; `write_coordinator_anchor` independently permits its control-state writes. Explicit `null` selects the ordinary writer. No enrollment, repair, silent fallback or whole-set reservation is implied. Native source/stage/history-root protection and exact per-writer database checks remain shared SDK/application responsibilities. Response overflow retains the original selection, never an invented latest seal.

### Held release sessions

The three [release-session tools](releasing.md) share the CLI's `gazette release-session` SDK owner, not command output or a second release engine. Open first and obtain authorization for the actual reported generation. Operations retain that writer; independently selected Git/authority profiles remain fixed. Missing optional profiles refuse only dependent operations. The selected Git profile supports file remotes or explicitly permitted HTTPS, with TLS verification and no redirects; SSH is unsupported. If HTTPS credentials are selected, they come from one explicitly permitted credential helper. An optional `AuthorityCommand` acquires public authority through its exact command, input and limits. Selecting command execution does not establish the provider's authority or freshness; those remain the caller's responsibility.

Four process-local handles are supported. Registry access is nonblocking. An operation looks up its handle and releases the registry lock before acquiring that session's own nonblocking lock. Concurrent operations on that same session are refused; independent held sessions do not share a whole-operation lock. Opening a new handle and removing a closed one still require registry access. There is no expiry, eviction, automatic retry or implicit close. Explicit close removes a handle even on native close failure; service drop preserves dirty custody. A prebounded shared `Delivery` plus handle precedes acquisition/effects. After successful open, readiness overflow retains the handle and returns that fallback. Historical readers remain separate and do not acquire sessions.

Discovery derives nested inputs/results from the shared SDK types. Native read and dispatch `timeout` uses `{ "secs": 30, "nanos": 0 }`; authority-command `timeout_ms` is a separate process-selection field. Native permissions still own every effect. Read the [release chain](releasing.md) for authorization, dispatch and uncertain-outcome recovery.

### Supplied release verification

`verify_release_authorization` calls the same pure verifier as the CLI. Supply authorization and binding as exact JSON strings: this preserves original bytes, duplicate decoded keys and numeric spellings for the native parser to validate. The binding supplies independently admitted public keys, observed time and exact expectations. The tool does not acquire their authority or freshness.

Success returns hashes, counts and the supplied observation time, not keys, destinations, signed bodies or private diagnostic details. Failures expose stable codes. Native byte/node/depth/target limits and MCP frame/result limits apply. This operation reads no files or clock, performs no signing, consumes no authority and makes no store or destination change. Verification is not release approval.

### Existing artifacts

`verify_release_commit` and `inspect_release_custody` require `allow_native_reads: true` and an explicit absolute `root` before native access. They never create, repair, retain, fetch, sign, synchronize or publish an artifact. Independently expected identities must not be learned from the suspect bytes being checked.

`verify_release_commit` and `inspect_release_custody` use the shared [commit-recipe decoder](../crates/gazette-core/src/release/commit.rs). `metadata_json` remains an exact JSON string until bounded native decoding so duplicate keys, positional forms, unknown fields and missing `parent` cannot be silently normalized by a transport object. `parent: null` explicitly selects a root commit. The default recipe limit is 16 KiB; `max_metadata_bytes` changes that limit independently of complete object/content `limits`. Discovery embeds the native recipe's `contentSchema`; parsing and native validation still own acceptance. Git attribution and explicit timestamps are recipe inputs, never publisher identity, authorization or the current clock.

Commit verification covers the complete selected object tree, not a mutable worktree. Custody inspection additionally requires the exact existing non-symbolic Gazette custody ref; it cannot establish prior synchronization, hardware durability or a release receipt. Both return the same native reports as direct SDK/CLI operation. A refusal or MCP response ceiling returns no partial successful report and changes no native state.

### Local commit preparation and custody

`prepare_release_commit` and `retain_release_commit` are separate effectful tools. Both require explicit native-read permission, the operation's independent write permission and absolute native roots before access. The required `protected_roots` array declares the complete selected protection set, including an explicit empty array when none are selected; Gazette does not infer missing source roots. `expected_tree` and nested native limits require named objects. Both use the exact `metadata_json` decoder and embedded discovery schema described above.

Preparation captures the selected existing output and checks its independently retained tree identity before creating detached Git objects. It neither retains custody nor updates a branch. Retention verifies the independently selected existing commit and creates/synchronizes only its exact immutable custody ref; `storage_bytes` bounds repository synchronization. It never implicitly prepares objects. Existing custody cannot be repointed to a different commit. Native limits, separation and read-back remain SDK/application/adapter responsibilities.

The shared SDK release-commit output helper reserves enough response capacity for the complete native success or failure before either native operation. Successful wire values remain `CommitReport` and `CustodyReport`. Native failure retains its reason and exact effect, including possibly remaining objects or references. A diagnostic exceeding the helper's explicit bound has a distinct failure code and original reason digest without weakening its effect classification. Insufficient reservation fails before native mutation; it never invokes the operation and then substitutes a generic output-limit success. Both MCP projections carry the same encoded native result, without rereading, retrying or reconstructing a new pin.

These tools advertise non-read-only, destructive, non-idempotent behavior. Successful local retention is not a journal receipt, publisher authorization, hardware power-loss qualification or deployment. Lost response delivery does not undo native effects: preserve the original request and reconcile using `verify_release_commit` or `inspect_release_custody` before further work.

## Observation tools

| Tool | What to pass | What it returns |
| --- | --- | --- |
| `observe_release_targets` | Original `targets_json`, absolute `program` and `isolation`, `allow_read`, optional network permission and limits | Report for all requested refs, including unavailable or indeterminate ones. Does not settle release. |
| `record_release_observation` | Absolute journal `root`, `expected_header`, new `id`, original `targets_json`, absolute `program` and `isolation`, independent permissions and limits | Session open, append and close outcomes separately. Keeps the original IDs and locations if the result is too large. |
| `inspect_release_observation` | Absolute database `path`, `id`, `allow_read`, `read` and `limits`, optional `expected_header` | Historical record or absence, original creation coordinates and current journal observation. Does not contact the destination. |
| `observe_github_pages_build` | Request JSON with pinned curl, one public Pages build and read/process/network options | Observation of that build, including pending, mismatch, denial or unavailable status. Does not infer the latest build, deploy or settle release. |
| `observe_delivery` | Request JSON with independently pinned output files, requested routes, curl and HTTP permissions | Byte comparison for each route, with mismatch and unavailable outcomes. Does not rebuild, retry, publish or settle release. |

### Explicit destination observations

`observe_release_targets` projects the same [native observation path](releasing.md) as the CLI. `targets_json` preserves the exact target-array bytes for duplicate-key and full-set validation. It is not a shell-command tool: the Git adapter owns fixed arguments, supported transports and strict response parsing. The explicitly selected executable and empty private isolation directory remain trusted native dependencies; the process is not sandboxed. `allow_read` acknowledges that invocation, and HTTPS additionally requires `allow_network`. No implicit credentials are acquired.

Every selected ordinal is retained. An unavailable row is part of a successfully delivered complete report, not proof that the destination matched. No advertised ref remains indeterminate. One aggregate native target-set deadline, per-target response bounds and MCP frame/result limits apply; cancellation or lost delivery does not undo an admitted read or authorize a push. The tool neither consumes authority nor writes release history. Output schemas derive from the actual native report types.

### Managed observation history

`record_release_observation` calls the shared SDK managed session exactly once: validate permissions, acquire the expected local generation, observe every target, append immutable history, and close the writer. `root` selects the existing private journal directory; it is never initialized or repaired. `targets_json` stays an exact JSON string until the native parser rejects duplicate/unsupported shapes. Its embedded content schema derives from native targets, not another target model. `protected_roots` names the complete absolute source roots to preserve; the SDK also protects the selected private isolation directory.

Optional `coordinator` names an existing independent `anchor_root` and exact retained native `binding`. Separately enable `write_coordinator_anchor`; selecting the guard does not grant that permission. The shared application protects both custody roots, requires the exact canonical journal root and identity/epoch match, then the SDK selects the existing native guarded writer. Missing/null selection uses the ordinary writer. Selected guard failures never fall back. The guard detects journal-only rollback and unresolved local lifetimes, not cross-host or whole-system rollback.

Independently enable `allow_read`, `record_observation`, `advance_writer_generation`, `reserve_local_coordination`, and `read.allow_coordination_files`; supported network destinations additionally need `allow_network`. All default denied. `read` exposes native storage/connection ceilings; `limits` bounds the complete target contract; `timeout_ms` bounds the complete sequential target observation while each reader receives only its remaining budget; `max_response_bytes` applies independently per destination.

The result is the SDK's borrowed `ObservationSessionReport`, including full native readbacks if append fails and the durable receipt if close fails. Before execution, MCP reserves enough result space for original `root`, `id`, input expected header and any selected coordinator anchor/binding. If even that cannot fit, no session starts. If the full result later overflows, the fallback says selected anchor writes, generation advancement, reads and insertion may have occurred. It never invents a current header. Retain request coordinates before invocation; carrier loss or cancellation does not undo blocking work. Never automatically retry. An existing ID refuses resampling, but acquiring the managed session can already have advanced a generation. Recording is therefore non-idempotent and effectful.

`inspect_release_observation` separately requires `allow_read` and `read.allow_coordination_files`, selects the database-file `path`, and invokes no Git process or network. Supplied `expected_header` is compared exactly; omitted or null means observation without that comparison. Native output distinguishes record creation coordinates from current journal coordinates. This can inspect history without an active signature; it does not establish remote freshness, full visibility, leases, anti-rollback fencing, authorization consumption, attempt linkage or settlement. Both tools have `readOnlyHint: false` because native coordination effects are possible. Only inspection is idempotent and nondestructive; annotations never grant permission.

## Recovery tools

| Tool | What to pass | What it returns |
| --- | --- | --- |
| `open_recovery_session` | Restricted SDK `OpenRequest` in `request`, including interrupted-operation details, snapshot, profile and permissions | Handle tied to the checked snapshot, or open failure with its original details. Does not open an ordinary writer or grant release approval. |
| `operate_recovery_session` | `session`, restricted SDK `operation` to inspect, reconcile, close or seal when permitted | Restricted recovery result. Cannot register, consume, dispatch or convert the writer. Refuses receive-pack history. |
| `close_recovery_session` | `session` | Close result. Removes the handle even on failed close. Does not settle release or restore writer access. |

For restricted recovery-session selection, operations and terminal settlement, follow [recovery](recovery.md). Recovery retains interrupted custody; it does not silently retry release dispatch.

## Required CLI and LSP coverage

When implementing another interface, use the [shared SDK and application](architecture.md#share-behavior-across-interfaces) rather than reconstructing command behavior from display text. The operation schemas above describe client requests; LSP has its own [protocol projection](lsp.md).
