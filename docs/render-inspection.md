# Inspect a local build

Read the output inventory from a local render, review the page model before templating or scan generated files for private values.

| If you want to… | Use |
| --- | --- |
| Render one site and inspect its inventory | [Execute and inspect](#execute-and-inspect) |
| Read content and relationships without rendering | [Inspect semantics](#inspect-semantics-without-rendering) |
| Build a private registry for tooling | [Project the complete private registry](#project-the-complete-private-registry) |
| Review page bodies before templating or attach a response | [Review pages before running the Theme](#review-exact-retained-surfaces) and [match a review response](#match-a-review-response) |
| Check for strings that should stay private | [Output disclosure](#private-output-disclosure-probe) |

## Execute and inspect

For a single site, the direct command reads the complete workspace TOML from stdin. Use the [site-set recipe](local-rendering.md#local-site-sets) when the workspace has includes or several sites must share a model:

```sh
gazette render-site --local --site site:one \
  --model-output machine/model.json < workspace.toml
```

Without `--output-parent`, Gazette reads sources and compiles in memory, with any explicitly permitted native generation. It returns private source/render evidence and the complete output inventory. `--local` is mandatory; draft and archived objects are not silently filtered, and site deployability is not publication permission.

Add `--output-parent /absolute/existing/private/stages` to create and retain a fresh private child. It never replaces an existing tree. The stage writer checks separation from every consumed native repository, writes the whole tree and verifies read-back before retention. The returned directory is the actual retained location; inspect it before retrying if receipt delivery fails. No deployment, signature, Git commit/push, database initialization or migration occurs.

### Keep acquisition, syntax and resolved state

Rust callers can keep each phase: `build::acquire_publication` → `AcquiredInputs::parse_publication` → `ParsedPublication::resolve` → `build::render_resolved_publication` → optional `build::stage_publication` and explicit native-guard `retain`. `ParsedPublication` owns the corpus and borrows the acquisition that supplied its bytes; it cannot be paired with a different workspace. `ResolvedPublication` borrows that syntax and holds the validated semantic model. It can be inspected or rendered without reparsing content, re-resolving its graph or reopening native sources. Presentation validation and compilation run when rendering is requested.

The convenience `build::render_publication` uses this same parse/resolve/render path. CLI code contains neither semantic compilation nor staging rules. Captures can be reused after native files change without changing their retained bytes; a new acquisition must verify the newly observed source. These retained phases do not implement admission or discoverability filtering.

### Inspect semantics without rendering

Check the retained publication semantics without rendering:

```sh
gazette check --stdin --publication site:one < workspace.toml
```

This captures the configured publication sources and selected presentation inputs, then parses the publication corpus, resolves its complete graph/routes/references and validates source-to-publication homes. Presentation bytes are captured and pin-checked, but this command does not validate presentation manifests, compile templates or resources, render bodies or create a stage. A valid semantic report can therefore precede a presentation or rendering failure. No native writes occur.

The `resolved-unadmitted` report includes the complete graph and private acquisition evidence, including drafts and unlisted/private values. It is operator evidence, not public output, a policy verdict or certification. Source acquisition and semantic failures still reject the operation; the command does not silently return a successful subset.

### Project the complete private registry

For the complete private registry projection of those same resolved inputs:

```sh
gazette check --stdin --publication site:one --registry < workspace.toml
```

`--registry` is mutually exclusive with `--review`. Rust callers use `ResolvedPublication::registry(RegistryLimits)`. This prepares typed catalogs, site/brand/publisher links, placements, the existing content graph, compiled routes/aliases and exact source associations. The consuming site does not filter this view: foreign sites, unused catalogs, disabled placements, drafts and archived content remain represented. Empty content still retains configured sites and catalogs. This is private structural evidence, not a persisted registry head, public admission or a store connection.

The `gazette.registry/v1` report remains `resolved-unadmitted`. `semantic_sha256` identifies the normalized structural model, using deterministic typed JSON field order and sorted collections. It is not a full build identity: body/asset bytes, presentation, native paths, source pins and operational state are excluded. Exact original workspace and full-document bytes have separate `configuration_sha256` and `documents` evidence. No native input is reopened during projection. The digest is not a release signature or authorization format.

`--max-registry-bytes` requires `--registry` and bounds the complete compact JSON report, including its exact-input evidence (default 16 MiB). Pretty-print display whitespace is outside that limit. Exhaustion returns no successful subset or partial check report. Acquisition, corpus and graph limits apply independently.

Persistence is a separate effect, not another mode of `check`. The SDK's `build::reconcile_registry` consumes this retained publication; the CLI's `reconcile-registry ROOT --site site:one` acquires and resolves the same inputs before invoking it. Complete registry replacement and writer-generation advance require separate explicit permissions. `inspect-registry PATH` observes the stored rows without reconciliation or migration. See the exact flags and [store guide](store.md). Neither operation turns this local unadmitted model into approved public output.

### Review pages before running the Theme {#review-exact-retained-surfaces}

Run this to inspect the page model and Markdown bodies before the Theme runs:

```sh
gazette check --stdin --publication site:one --review < workspace.toml
```

The result includes Markdown body fragments, the site graph, local page contexts, drafts, nested foreign references, every configured static file, effective composition and paths of uncompiled opaque inputs. A broken body reference can fail this step even when structural checks pass. Static file descriptors identify bytes without checking binary safety. This command does not run Theme or resource programs or write output. To inspect the final HTML or other files, [render the site and scan its output](#private-output-disclosure-probe).

Mathematical sources also need complete source-bound typesetting results. Reuse the `math.results` object from an explicitly authorized native render; review does not execute the typesetter. For a local workspace, extract that object and supply it on standard input:

```sh
python3 -c 'import json,sys; json.dump(json.load(sys.stdin)["math"]["results"],sys.stdout)' \
  < authorized-render.json > retained-math.json
gazette check --workspace-root "$WORKSPACE" --publication site:one --review \
  --retained-math-stdin < retained-math.json
```

Keep the complete result map for the captured corpus, including its original logical paths. The existing `ParsedPublication::with_math` admission checks the current body digests, profiles and every formula occurrence; the formatter checks emitted MathML. Extra, missing or stale results fail. Omitting required results returns `publication_review_math_required`. `--retained-math-stdin` requires `--workspace-root` and `--review`, and bounds its complete JSON input by `--max-review-bytes` (16 MiB by default), independently of review output. This supplied data is not an attestation of its producer or permission to publish.

The report contains `sha256` and `request`. The latter's `gazette-publication-review/1` format binds the selected site, exact configuration digest, captured source-tree identities and ordered typed surfaces. Its `selection_sha256` is null for this unselected inventory; the [selected Rust review](releasing.md#closed-candidates) instead binds full candidate intent. `sha256` hashes compact request JSON, not the outer report. This is an Engine-owned inspection encoding, not a publication policy or release authorization format. Configuration comments and machine paths affect this exact request identity; do not use it as the portable identity of rendered artifacts. Neither digest equality nor a successful inventory is an owner approval. Keep the report private.

Rust callers use `build::review(&resolved, ReviewLimits)` with the SDK's native adapters, or `ResolvedPublication::review` with explicit raster/Markdown adapters; `request().surfaces()` exposes immutable values from the shared projections. Each surface's `uses()` iterator gives every scalar/container occurrence with an RFC 6901 pointer. The same subject appearing on a page and in the site graph has different use contexts, and an allowed page must not conceal a held nested value. `--max-review-bytes` bounds compact request JSON and, independently, aggregate pointer bytes (default 16 MiB); `--max-review-uses` bounds all occurrences (default 100,000). Both flags require `--review`. Exhaustion returns no partial request. Later admission and immutable-output review must still account for compiled resources, rendered Theme text and disclosure across the output tree.

### Match a review response

Rust integrations can compare a response with that retained request using `PublicationReview::match_response(bytes, &ReviewResponseBinding, ReviewResponseLimits)`. The binding supplies the expected `ReviewPolicy` (`id`, `revision`, `source_sha256`) and `response_sha256` independently of the bytes being checked. Deriving the expectations from the same response supplies no independent proof. This operation performs no I/O, policy execution or clock reads; there is not yet a CLI response-import command.

The strict `gazette-review-response/1` JSON transport contains `format`, `request_sha256`, `policy` and `decisions`. Each decision has a zero-based `surface` index in the exact request, its canonical `pointer`, an `outcome` (`allow`, `hold` or `exclude`) and a nonempty list of unique policy-native `evidence` keys. Every occurrence, including each root and nested container, needs exactly one row. Parent decisions, wildcard/prefix paths and normalized array indexes cannot stand in for missing children. Reported order is preserved; changing whitespace or row order changes the response-byte pin. Unknown or duplicate JSON fields, positional root/policy/decision arrays, object-form outcomes, unsupported format, mismatched pins/policy, missing/extra uses and invalid evidence references reject the complete match.

`ReviewResponseLimits` defaults to 16 MiB of raw input and 100,000 decoded decisions. The byte bound applies before hashing/decoding; the record bound applies before coverage matching. A successful result remains `matched-unverified`, including when every outcome is `allow`. Evidence keys are references, not verified records; the match does not authenticate the evaluator, evaluate freshness/revocation, approve rights or certify disclosure. An `exclude` row does not authorize silently deleting content, and a held child is not overridden by an allowed container. No renderer or release operation accepts this result as approval. The owned result retains the original `response_json` and request digest after the source request is dropped. Keep it private. The [local preparation recipe](releasing.md#build-a-local-preparation-request) can carry complete per-site responses through candidate checking, immutable output checking and durable read-back; the ordinary unguarded local-render recipe refuses response input.

## Which failures stop a render {#output-and-failure-boundaries}

Missing routes, missing alias selection, output collisions, parser/template errors and resource ceilings return no successful partial tree. Output options bound entries, depth, per-file and aggregate bytes. Input acquisition currently uses 256 source keys and the native source defaults. `PublicationRenderLimits` separates corpus and graph limits from `PublicationOutputLimits`, which bounds subsequent presentation metadata, template rendering, resource compilation and capability metadata. Rendering an existing `ResolvedPublication` accepts only those post-resolution limits; it does not rerun semantic resolution under different limits. Native output inventory ceilings remain in `SiteRenderOptions`. Rust callers can choose these limits explicitly; they are not a CPU or hostile-template sandbox.

Supported scope and limits:

- Capability degradation remains a closed, typed choice. Only the documented `no-native-acquisition` outcome can produce typed `unavailable`; execution failure, stale evidence and unsupported selected input still fail the site.
- Opaque files become output only through an explicit supported static-asset or resource selection. Other sites' retained assets remain evidence, not this site's output obligation, and unsupported selected input fails rather than being copied or omitted.
- Unframed Markdown still requires the explicit project profile. YAML metadata, unrestricted raw HTML, active notebook HTML/widgets and implicit executable or asset discovery remain unsupported by this profile. Typed resource recipes, bounded raster transformations, static notebooks and explicit route mirrors use their separately documented paths instead of weakening Markdown admission.
- Candidate admission, technical qualification, disclosure checks, durable local preparation and release/recovery are connected successor phases. They do not authenticate owner rights/privacy evidence, grant publisher approval or prove a hosted-platform build; those authorities remain external to local rendering.

## Scan the output for private values {#private-output-disclosure-probe}

<a id="media-selection-and-json"></a> <a id="html-channels"></a> <a id="javascript-channels"></a> <a id="css-channels"></a> <a id="xml-channels"></a> <a id="encoded-candidates-and-embedded-coverage"></a> <a id="probe-report-and-comparison-limits"></a> <a id="required-supported-profile-check"></a> <a id="typed-disclosure-policy"></a> <a id="raster-metadata-in-policy-checks"></a> <a id="direct-cli-inspection"></a>

Use [Output disclosure checks](output-disclosure.md) to scan generated files for private values, or [require supported formats](output-disclosure.md#required-supported-profile-check) before preparing a release.

[Back to local rendering](local-rendering.md).
