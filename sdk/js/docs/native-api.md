# Gazette Node API reference

Use this reference when embedding Gazette in a Node application. Start with the [complete configuration example](../README.md#check-a-configuration-and-locate-a-field) to load the addon and interpret a result. The examples below illustrate individual calls; publication recipes, retained trees and journals are inputs supplied by your application, not objects this SDK creates implicitly.

The [TypeScript declarations](../index.d.ts) describe the JavaScript types; the Rust SDK defines the JSON request and response shapes. To build the addon, use [the package setup](../README.md#build-and-load-the-local-addon).

## Find a method

| You need to | Methods or contract |
| --- | --- |
| Check configuration and locate its fields | [Configuration example](../README.md#check-a-configuration-and-locate-a-field): `checkWorkspace`, `fieldOrigin`, `compositionOrigin` |
| Use the package from TypeScript | [Package contract](#typescript-package-contract) |
| Inspect a page and its supplied companions | [`inspectPagePackage`](#supplied-page-package-inspection) |
| Inspect supplied text | [`inspectMarkdown`, `inspectDocument`](#supplied-markdown-and-framed-documents) |
| Inspect a file from a pinned source tree | [`inspectMarkdownTree`, `inspectDocumentTree`](#explicit-pinned-tree-inspection) |
| Render publications and obtain their files | [`renderSites`, `renderSiteFiles`](#private-local-publication-rendering) |
| Assemble developer documentation | [`assembleDocumentation`](#documentation-repository-assembly) |
| Review content before rendering | [`reviewPublication`](#private-publication-review) |
| Run the configured qualification checks | [`qualifySites`](#complete-set-technical-qualification) |
| Prepare local release output | [`prepareSites`](#complete-local-preparation) |
| Compare served pages with retained output | [`observeDelivery`](#observe-served-output) |
| Read recorded release history | [`inspectReleaseHistory`](#immutable-release-history-inspection) |

## Shared input and result conventions

All calls are synchronous. Supply text as `Uint8Array` bytes (including Node buffers), and queries use the Rust SDK `FieldQuery`/`CompositionOriginQuery` JSON contracts. Source spans are half-open UTF-8 byte ranges, not JavaScript UTF-16 string offsets. Configuration calls resolve the complete supplied workspace; no default or inferred field receives an invented origin. Text-inspection calls parse only the supplied Markdown or explicitly framed document.

Inspection results are exact native JSON **strings**; `renderSiteFiles` and `assembleDocumentation` additionally return owned binary payloads as described below. The addon does not turn native integers into JavaScript numbers. Use a lossless JSON reader when needed; ordinary `JSON.parse` is the caller's numeric-conversion decision. The included TypeScript declarations describe this string boundary, not an independently generated complete workspace schema.

## TypeScript package contract

The package exposes native ESM through its `import` export and explicit declarations through `types`. It is not a CommonJS-only package: the internal `createRequire` loads Node's native addon, while consumers use ordinary ESM. There is no separate TypeScript compiler or configuration implementation.

Pass complete workspace TOML to this function. The [configuration example](../README.md#check-a-configuration-and-locate-a-field) shows a minimal valid workspace and how to interpret its result.

```typescript
import { loadGazette, type NativeJson, type ProjectionLimits } from '@rtepl/gazette';

const gazette = loadGazette('/absolute/local/gazette.node');
export function checkConfiguration(sourceToml: string): NativeJson {
  const source: Uint8Array = new TextEncoder().encode(sourceToml);
  const limits = { sourceBytes: source.byteLength } satisfies ProjectionLimits;
  return gazette.checkWorkspace(source, limits);
}
```

`Gazette` declares the frozen function properties as readonly. `NativeJson` is a string alias, not proof that an arbitrary caller-created string passed native validation. Query strings still use the Rust query grammar; declarations do not pretend to validate their embedded JSON or expose a second typed workspace model. Safe-integer, UTF-8 and semantic checks remain native runtime responsibilities. No ambient Node types are required to consume these declarations: the public boundary uses standard strings, numbers and `Uint8Array`. Consumers choose their own host declarations for APIs such as `TextEncoder` or Node buffers.

## Supplied page-package inspection

`inspectPagePackage(request, options?)` projects the Rust SDK authoring session's existing `PagePackageOwnership` result from caller-supplied workspace, document and companion bytes. It performs no filesystem discovery or source acquisition and does not independently resolve resources.

```typescript
const pagePackage: NativeJson = gazette.inspectPagePackage({
  workspace: new TextEncoder().encode(workspaceToml),
  documents: [{ path: 'guide/index.md', bytes: framedDocument }],
  companions: [{ path: 'guide/image.png', bytes: imageBytes }],
  query: 'guide/index.md',
});
```

The result is the exact native optional ownership JSON: a matched authored page contains the application-owned source, content, resolved resources and package digest fields, while a standalone document serializes as `null`. Paths and bytes are explicit inputs; the JavaScript layer neither probes the live filesystem nor adds a compatibility resolver. Duplicate or invalid supplied paths and native authoring failures throw bounded `{code}` errors.

`selectionEntries`, `identifierBytes`, `sourceBytes` and `resultBytes` are independent safe-integer ceilings. The defaults are 4,096 entries, 16 KiB per identifier, 16 MiB across workspace/document/companion bytes and 1 MiB for the serialized result. The operation is synchronous and retains no session handle.

Workspace failures throw an `Error` whose message is the serialized shared Rust SDK `workspace::Failure` (code, explanation and available diagnostics/span). Treat these as private: they can contain supplied identifiers. Host failures use `node_limits`, `node_source_utf8`, `node_query`, `node_query_limit`, `node_input_limit`, `node_result_limit` or `node_encoding`. Host type-conversion failures are ordinary Node-API errors and need not have JSON messages.

Optional `ProjectionLimits` preserve native TOML defaults when omitted. Query JSON defaults to 16 KiB and serialized results/failures to 1 MiB. Numeric options must be nonnegative safe integers. Source bytes are bounded before copying and parsing; query bytes before JSON decoding; the shared Rust writer bounds each serialized write before buffering. These are logical per-call limits, not an OS memory/CPU sandbox. Calls are synchronous and introduce no unbounded worker queue.

## Supplied Markdown and framed documents

`inspectMarkdown(bytes, options?)` returns the shared Rust SDK's `{inspection, html?}` JSON. `inspectDocument(bytes, options?)` returns `{document, html?}` and requires explicit `+++` TOML delimiter lines. These are separate entrypoints: neither filenames nor source contents select a different parser implicitly. The SDK parses once and returns the complete requested report.

```typescript
const markdown: NativeJson = gazette.inspectMarkdown(source, {
  html: true,
  limits: { sourceBytes: source.byteLength, factBytes: 1_048_576 },
  resultBytes: 1_048_576,
});
const framed: NativeJson = gazette.inspectDocument(framedSource, {
  html: true,
  sourceBytes: framedSource.byteLength,
  metadata: { sourceBytes: 65_536 },
  markdown: { outputBytes: 1_048_576 },
});
```

`MarkdownLimits` exposes the native `sourceBytes`, `nodes`, `depth`, `factBytes` and `outputBytes` ceilings. Document options separately expose the complete `sourceBytes`, TOML `metadata` byte/node/depth ceilings and `markdown` body limits. Every omitted field uses its native SDK default. `resultBytes` independently bounds complete serialized results and defaults to 1 MiB; logical projection bounds are not interchangeable with serialized JSON length.

Exact UTF-8 bytes are checked before copying and parsed without newline or BOM normalization. Markdown ranges are body-relative; a document also reports the body's full-source span. The addon does not turn these into UTF-16 coordinates. Optional HTML uses the SDK's existing safety rules. Raw HTML, unsafe URLs, framing or any other requested-projection failure throws without a partial inspection. No themes, source acquisition, publication rendering or release are implied.

These two operations expose native failures as redacted JSON `{code}` messages; they do not echo private source, metadata or provider diagnostics. Pre-copy source ceilings retain `markdown_limit` or `document_limit`. Invalid UTF-8 and invalid numeric/result bounds use the existing host classes. Successful inspection itself deliberately contains supplied source-derived text and must still be treated as private data.

## Explicit pinned-tree inspection

`inspectMarkdownTree(treeJson, permission, options?)` and `inspectDocumentTree(treeJson, permission, options?)` expose the same native complete-tree acquisition as CLI/MCP inspection. They do not replace the supplied-byte operations or infer framing from a filename.

`treeJson` is the exact Rust [`markdown::input::Tree`](../../rust/src/markdown/input.rs) selection: absolute `root`, independent complete-tree `identity`, confined regular-file `document`, and optional native `limits`. No pin is inferred from the selected page. All entries, including hidden/companion assets, participate in acquisition and pin verification before parsing the retained document. No fallback, directory scan outside the selected root, or second file read occurs.

```typescript
const inspected = gazette.inspectMarkdownTree(exactTreeJson, {
  allowRead: true,
  queryBytes: 16_384,
}, { html: true, resultBytes: 1_048_576 });
```

`TreeOptions.allowRead` is mandatory; false refuses native access. `queryBytes` defaults to 16 KiB and bounds selection JSON before decoding. Duplicate fields, positional/null native maps, unknown fields, relative roots and escaping document paths are rejected. Capture ceilings in `treeJson.limits`, parser/HTML ceilings in the existing `MarkdownOptions` or `DocumentOptions`, and complete serialized `resultBytes` remain independent. Tree identity and native integer lexemes cross as exact JSON, not a second JavaScript schema.

Success adds the shared `input: {identity, document}` evidence to the existing inspection/document report. Omitted HTML stays omitted. Errors expose bounded native `{code}` classifications; permission refusal is `markdown_read_permission`. Read access and matching bytes are not publication approval. Calls are synchronous and return no retained handle; previously returned JSON remains original evidence, while another call must acquire and validate its inputs again.

## Private local publication rendering

`renderSites(recipeBytes, options)` consumes the Rust SDK's exact `LocalPublicationRecipe` JSON (`gazette-local-publication-set/1`), including the complete workspace TOML and explicitly selected site render choices. This is the same recipe as CLI local rendering and MCP `render_sites`, not another workspace schema. Both `local: true` and `allowRead: true` are required before acquisition.

Optional recipe `math_generation` and `diagram_generation` each separately permit and bound their configured native generator using `allow_execute`. Omitting either performs no corresponding acquisition. Native commands are not sandboxed; their effects are not covered by source-read permission and a failed or interrupted invocation must not be blindly retried. Rendering consumes the retained results without invoking the command.

```typescript
const report: NativeJson = gazette.renderSites(recipeBytes, {
  local: true,
  allowRead: true,
  inputBytes: recipeBytes.byteLength,
  resultBytes: 8 * 1024 * 1024,
  limitsJson: '{"acquisition":{"sites":2},"output":{"total_bytes":1048576},"max_total_output_bytes":2097152}',
});
```

`inputBytes` defaults to the native TOML source-byte ceiling; `limitsBytes` defaults to 16 KiB; `resultBytes` defaults to 1 MiB. These safe-integer host ceilings are independent. Recipe bytes are checked before native decoding. `limitsJson` is a bounded, duplicate-safe named JSON object. Its optional fields compose existing Rust types without coercing their integers through JavaScript:

| Field                      | Native owner                                 |
| -------------------------- | -------------------------------------------- |
| `syntax`                   | `toml::Limits`                               |
| `acquisition`              | `build::PublicationSetAcquisitionLimits`     |
| `output`                   | `source::Limits`, applied per site           |
| `max_total_output_bytes`   | `build::PublicationSetRenderLimits::bytes`   |
| `max_total_output_entries` | `build::PublicationSetRenderLimits::entries` |

Omitted fields retain native defaults. Unknown fields, positional objects, null objects, duplicate keys and invalid native integers are rejected before source reads. Nested `acquisition.sources`, when supplied, requires the native `PreparationLimits` fields; it is not a partial patch or an override language.

The Rust SDK captures the exact pinned sources once and renders the whole set through its shared model, resource compilers and Theme. Success returns the full `PublicationSetInspection`. A failed sibling throws that same complete inspection as `Error.message`, preserving every success and failure; it never becomes a smaller successful set. Earlier native failures expose only `{code}`. Complete reports are private and can contain source paths, authored text and diagnostics. Result exhaustion throws `node_result_limit`, never a truncated JSON report.

This operation returns evidence, not generated file bytes or a persistent handle. Its retained inputs and outputs are released after serialization. It performs no staging, policy subprocess, preview listening, database access, signing, network publication or certification. Synchronous native limits bound logical work, not wall-clock execution; callers must not mistake this for a cancellable worker.

### Deliver generated files to JavaScript

`renderSiteFiles(recipeBytes, options)` uses the same permissions, recipe, acquisition, compiler and complete-set checks as `renderSites`. It returns `{ report, sites: [{ site, files: [{ path, bytes }] }] }`. `report` is the same exact native inspection JSON string. Each `bytes` value is an owned `Uint8Array`, including for binary assets; no text decoding or Base64 transport is imposed. Site identities and paths follow native deterministic order. Directory entries remain in the report inventory and do not receive fabricated file payloads.

Native `max_total_output_bytes` and `max_total_output_entries` bound the complete generated set before delivery; `resultBytes` independently bounds report JSON. An incomplete site set throws its full inspection before any file delivery, never returns only the healthy sites. Changed returned buffers cannot alter source trees or later calls. The result itself is not deep-frozen, a filesystem stage, a persistent native handle, a certified release, or permission to publish. Both methods share one native render invocation per call; neither reruns the compiler to fetch files or uses CLI/MCP as an internal bridge.

## Documentation repository assembly

`assembleDocumentation(workspaceBytes, site, options)` returns Mintlify build files assembled from the selected site's repository manuals and imported or generated API reference. Supply complete workspace TOML as bytes and its declared site key.

The workspace selects each project's manual pages, companions, reference inputs and navigation. Gazette captures the selected source trees; only the selected pages and artifacts enter the output. Ordinary publication and Theme sources are not required by this operation. See [Mintlify integration](../../../docs/mintlify.md) for the build flow and [input formats](../../../docs/mintlify-formats.md) for compatibility and selection rules.

```typescript
const documentation = gazette.assembleDocumentation(workspaceBytes, 'site:example', {
  local: true,
  allowRead: true,
  limitsJson: '{"output":{"total_bytes":1048576}}',
});
const report: NativeJson = documentation.report;
const files = documentation.files; // PublicationFile[] with owned Uint8Array bytes
```

Both permission fields are required. `inputBytes` bounds workspace bytes before UTF-8 decoding (native TOML source-byte default); `limitsBytes` bounds the exact limits JSON, optional `generationJson` and optional `previewLinkBasesJson` individually (16 KiB default); `resultBytes` independently bounds the complete report (1 MiB default). Host ceilings must be nonnegative safe integers. Native `limitsJson` composes `syntax: toml::Limits`, `acquisition: source::PreparationLimits` and `output: source::Limits`. Omitted sections use existing SDK defaults; `acquisition` uses the documentation pipeline's publication-acquisition source defaults. A supplied acquisition object uses its native required fields, not a partial patch. Duplicate keys, positional/null maps and unknown fields are rejected before acquisition. Native integer values remain exact JSON, not JavaScript numbers.

For private preview links to product help, `previewLinkBasesJson` supplies the shared public-base to literal-loopback-base map as a JSON object. Malformed or non-object JSON refuses with `node_query`; the native routing contract validates the selected bases. The [local preview-link contract](../../../docs/mintlify.md#local-preview-links) owns its path-preservation rules and private-output limits. Omit the map when assembling ordinary public destinations.

The result is `{report, files}`. `report` is the existing Rust `build::documentation_output::Report` serialized without a competing schema: exact captured input/configuration evidence, brand-selected repository, separate `manual` page/companion evidence, reference `artifacts` and the complete target inventory. `directory` is null because the assembled target is not staged. `jobs` is empty when no generators run. File payloads reuse `PublicationFile`, in native path order; directories remain in the report. The target's complete output limits apply before any file delivery. Returned buffers cannot mutate native sources or later results. Binary-safe delivery does not imply the selected target accepts arbitrary binary companions.

Any failed required manual or reference input rejects the entire operation. Before effects and when no generator runs, native failures expose bounded `{code}` messages; permission refusal is `documentation_permission`, invalid site spelling is `documentation_site`, and transport errors retain the existing `node_*` classes. Configured native-generator inputs retain the SDK's `documentation_generation_required` classification when generation is unselected; there is no automatic execution or substitution of imported bytes.

To run the workspace's configured generators, additionally select `allowStage:true` and a strict named `generationJson` request:

```typescript
const generated = gazette.assembleDocumentation(workspaceBytes, 'site:example', {
  local: true, allowRead: true, allowStage: true,
  generationJson: '{"parent":"/absolute/existing/private-jobs","max_jobs":2,"directory_bytes":256,"entry_bytes":64,"allow_execute":true}',
});
```

The existing absolute `parent` contains fresh input/output stages, rather than the documentation repository's final destination. Three required fields bound the generated jobs and their path names:

| Field | What it limits |
| --- | --- |
| `max_jobs` | Number of configured generated-reference jobs across the entire selected documentation navigation, from 0 through 128. Imported references and manual pages do not count. Zero is valid only when no generated jobs are selected. This is not a concurrency limit or a count of subprocess calls. |
| `directory_bytes` | Maximum native byte length of each full canonical stage path, from 1 through 1,048,576. It includes the parent, separator and generated `.gazette-stage-` name with 32 hexadecimal digits. Unix OS bytes are measured before directory creation. |
| `entry_bytes` | Maximum UTF-8 byte length of each selected output file or subdirectory's relative path, from 1 through 1,048,576. Selecting the capture-root directory supplies no relative path. This does not limit artifact contents or count entries. |

Source file sizes, inventory counts and other content limits remain separate. Optional `input` uses native `source::Limits`; `timeout_ms` defaults to 30,000 (1–300,000), and `max_capture_bytes` to 1 MiB (at most 64 MiB). `allow_execute` independently defaults false. `allowStage` alone never permits commands. The configured recipe owns executable pins, arguments, artifact selection and format; the JS request does not introduce a generator schema. Native commands are not sandboxed and can perform their configured effects.

The Rust SDK reserves complete custody response capacity before acquisition or stage creation, executes the selected jobs once, then assembles their retained artifacts through the same target adapter. All files are delivered together, with actual jobs in the native report. A later generator or assembly failure, or full-report overflow, rejects with the SDK's compact original custody JSON, not a bare code or successful subset. Inspect completed jobs, failure evidence and exact Unix stage-path bytes; do not automatically retry, clean up or infer rollback. If the native operation violates its reserved delivery bounds, `Error.message` contains JSON with `code: "documentation_delivery_contract"` and a `custody` debug string describing the jobs and stages. That string is diagnostic text, not a stable JSON schema, and this exceptional response can exceed `resultBytes`. A missing native file payload instead throws `node_output_contract`. Keep the original request and the complete error; inspect the reported stages and completed jobs before retrying or deleting anything. Host termination or allocation failure can prevent any JavaScript response, even after a generator has run or a stage has been created.

No final-target staging, database, signing, provider publication or public admission is performed. The configured destination is not created or replaced. Generator execution evidence alone does not qualify language fidelity or hosted compatibility.

## Private publication review

`reviewPublication(workspaceBytes, site, options)` returns the Rust SDK's complete `build::PublicationReview` as exact JSON. It uses the shared native acquisition, corpus parsing, graph resolution and review owners without calling the CLI or MCP. Workspace bytes are TOML and `site` is the exact configured acquisition site; neither is an ambient path or inferred selection.

> [!IMPORTANT]
>
> This Node method does not accept retained typesetting results. If the publication contains mathematics that needs those results, use the Rust retained-math API or the [CLI/MCP review path](../../../docs/render-inspection.md#review-exact-retained-surfaces). Review does not run a typesetter; `allowRead` cannot enable one.

```typescript
const review: NativeJson = gazette.reviewPublication(workspaceBytes, 'site:example', {
  allowRead: true,
  resultBytes: 8 * 1024 * 1024,
  limitsJson: '{"review":{"uses":100000}}',
});
```

`allowRead` is mandatory. Native sources are captured once and parsed/resolved through their existing owners. The full private review preserves source evidence, drafts, nested uses, assets, composition and uncompiled obligations. Native Markdown/notebook/raster projection runs; Theme/resource programs, policy processes, staging, databases, signing and network publication do not. Review identity is not approval, certification, permission to publish or a retained handle.

`inputBytes` bounds workspace bytes before UTF-8 decoding (native TOML source-byte default), `limitsBytes` bounds limits JSON (16 KiB default), and `resultBytes` independently bounds the complete JSON result (1 MiB default). Host ceilings are nonnegative safe integers. `limitsJson` composes existing Rust types: `syntax: toml::Limits`, `acquisition: source::PreparationLimits`, `corpus: corpus::Limits`, `graph: publication::content::Limits`, and `review: build::ReviewLimits`. Omitted sections use native defaults; acquisition uses the publication pipeline's source defaults. Its supplied object requires the native fields, not a partial override. Duplicate keys, unknown fields and positional/null objects at every nested limits boundary fail before acquisition. Native integer lexemes remain exact JSON rather than JavaScript numbers.

Any acquisition, corpus, graph or review failure throws a bounded native `{code}` message with no successful subset. Permission refusal is `publication_permission`; invalid site spelling is `publication_site`. Host failures retain existing `node_*` classes, including `node_result_limit` rather than truncated JSON. Successful review contains private source-derived values and must not be treated as a public-safe report.

## Complete-set technical qualification

`qualifySites(recipeBytes, { allowRead: true, allowPolicyExecution: true })` passes the exact `QualificationRecipe` JSON bytes to the Rust SDK operation used by CLI `qualify-sites` and MCP `qualify_sites`. The same captured candidates, selected native policy checks, rendering and output qualification produce the complete `QualificationReport`. Read permission is required independently of checker permission; omitted checker permission denies execution. Explicit native math/diagram generation requests retain their own permissions. Math executes its exact retained provider tree. Graphviz uses Gazette's file-free DOT profile, fixed arguments, empty environment and a verified disposable working directory; its ambient native libraries, plugins and fonts are still not a sandboxed or portable provider identity.

Only a complete technically qualified site set returns normally. Every other native outcome throws its complete JSON report, retaining all requested sites, including failed and not-run members. Neither qualification nor a certificate grants publisher approval, preparation, signing or deployment authority. This operation creates no Gazette output stage, store or Git preparation.

`inputBytes` and `resultBytes` default to 4 MiB; native acquisition, checker, rendering and evidence limits remain independent. A bounded failure envelope is reserved before execution. If the complete report cannot be delivered, the error has code `qualification_response_unavailable` and the SHA-256 of the exact input recipe bytes. Configured programs may already have run: retain the original request and inspect native outcomes, never infer rollback or retry permission. If even that envelope cannot fit, execution does not start. Catastrophic host failure, cancellation or lost transport cannot guarantee delivery of any report.

## Complete local preparation

`prepareSites(recipeBytes, options)` consumes the Rust SDK's existing `LocalPreparationRecipe` JSON. It captures and renders the selected site set, then performs explicitly permitted fresh staging, durable preparation and local Git worktree initialization through that same native operation. It does not initialize a journal, acquire publisher authorization, sign, push or deploy. The exact existing journal must be initialized separately with its own permission.

```typescript
const report: NativeJson = gazette.prepareSites(recipeBytes, {
  local: true, allowRead: true,
  createStage: true, recordPreparation: true,
  advanceWriterGeneration: true, reserveLocalCoordination: true,
  initializeWorktree: true,
  limitsJson: '{"read":{"allow_coordination_files":true}}',
  resultBytes: 8 * 1024 * 1024,
});
```

`local` and `allowRead` are required. Every optional effect flag defaults denied: `createStage`, `recordPreparation`, `advanceWriterGeneration`, `reserveLocalCoordination`, `writeCoordinatorAnchor`, `initializeWorktree` and `allowPolicyExecution`. They map to the native staging, record, writer/session, anchor, worktree and policy permissions, respectively. Coordination-file effects remain separately explicit in `limitsJson.read.allow_coordination_files`. The recipe's selected math execution uses its existing separate permission. Configured subprocesses are not sandboxed by this addon; native source-read or local-preparation acknowledgement does not authorize arbitrary subprocess effects.

`inputBytes`, `limitsBytes` and `resultBytes` retain the other publication methods' independent safe-integer defaults. `limitsJson` composes native `syntax`, `acquisition`, `output` and `read` types as duplicate-safe named JSON. Omitted fields retain native defaults; supplied nested acquisition values use their native required fields. `output` feeds both rendering and staging. Limits cannot override the top-level permission flags. Other native operation bounds retain their defaults; this is not a second recipe or configuration language.

Success returns the exact complete native report. Native failure throws that complete report in `Error.message`, including any completed prefix, failed stage, journal generation, checker or Git custody. The shared Rust delivery owner reserves the complete original reconciliation coordinates **before execution**. If the final native report cannot fit `resultBytes`, the error carries that reserved `reconciliation-required` envelope: original journal, expected header, coordinator selection and ordered site/preparation IDs. These are not current observations. A budget too small for the fallback throws `node_result_limit` before any native execution. No truncated report or smaller successful site set is returned, and no operation is automatically retried.

Keep the original request coordinates independently. Host termination, allocation failure or loss after return cannot establish rollback. Inspect native history before deciding further work. Preparation evidence remains private and is not publication approval, certification or a release receipt.

## Observe served output

`observeDelivery(requestBytes, { inputBytes, resultBytes })` consumes the exact Rust `delivery::Request` JSON used by CLI `observe-delivery` and MCP `observe_delivery`. The optional JavaScript options contain transport bounds only; both default to 1 MiB. The native request selects the existing retained output root and independent tree identity, exact routes/base URL, pinned curl, native limits and each `options.http` permission (`allow_read`, `allow_execute`, `allow_network`). No permission is implied by addon loading, a loopback address or a transport bound.

Every selected route is observed once. A complete match returns the native `delivery::Report` JSON string; differences, HTTP failures and unavailable responses throw that same complete report with no dropped routes. Whole-request refusals retain native stable error codes. Response body contents are not returned. These observations neither rebuild output nor establish publisher approval, unselected-route correctness, provider readiness or release settlement.

A request-bound `delivery_response_unavailable` envelope is reserved before effects. If the complete result exceeds its bound, HTTP requests may already have occurred; preserve the original request and do not infer rollback or retry safety. An envelope that cannot fit prevents native observation. No transport envelope guarantees delivery through catastrophic host failure or cancellation.

## Immutable release-history inspection

`inspectReleaseHistory(requestBytes, {allowRead:true})` exposes the Rust SDK's five existing history readers through one closed selector: `authorization`, `leases`, `consumption`, `attempts` or `reconciliations` in `kind`. The exact JSON request also contains absolute `database`, `record_id`, optional `expected_header` (the native `store::JournalHeader`, or null), and optional `read` (native `store::ReadOptions`). Native defaults apply; coordination-file permission must be explicitly enabled with `read.allow_coordination_files`. No request automatically selects the latest record, opens a writer or repairs a database.

`HistoryOptions.inputBytes` defaults to 16 KiB; `resultBytes` defaults to 1 MiB. Duplicate keys, unknown fields and positional maps are refused. Decode original request bytes without rounding native journal coordinates through JavaScript numbers. Results remain complete native JSON strings, including absent-record evidence; expected-header mismatch and native storage failures throw `{code}`.

Before reading, the operation reserves a bounded `history_response_unavailable` envelope with the exact request digest. If the complete report cannot be delivered, that envelope reports possible read/coordination-file effects, not partial history or success. Too little room even for the envelope prevents the read. Observation does not renew expired approval, consume authorization, resample a destination, retry delivery, settle a release or prove anti-rollback.
