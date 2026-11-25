# Browser SDK integration

Add search, consent-controlled analytics or read-only reader tools to a theme. These browser modules do not load the native addon. Use the [Node SDK](../README.md) to embed the publishing engine instead.

The three browser modules are licensed under the Apache License, Version 2.0, so a theme can bundle them into its pages on those terms. Keep each module's `LICENSE` and `NOTICE` with a copy. The rest of Gazette, including the Node SDK and its addon, stays under AGPL-3.0-or-later.

| You need | Browser module |
| --- | --- |
| Rank the publication's search records | [`search`](../search/index.d.ts) |
| Load analytics after consent | [`analytics`](../analytics/index.d.ts) |
| Expose public reading tools | [`webmcp`](../webmcp/index.d.ts) |

The workspace configures which features a theme receives. [Capability configuration](../../../docs/browser-capabilities.md) explains those bindings. Importing a module does not activate it or supply its controls.

## Search the emitted corpus

`@rtepl/gazette/search` is a separate browser-safe entry; it imports no Node-API addon and requires no DOM framework. It ranks Gazette's existing discovery-safe `search.json` records. It does not scan repositories, reconstruct relationships, decide public admission, build a second publication corpus or supply Theme UI.

This helper loads the URL supplied by the theme's `presentation.search.url` and returns a search index. Pass that exact URL as `searchUrl`; the SDK does not guess an endpoint.

```javascript
import { loadIndex } from '@rtepl/gazette/search';

export async function loadPublicationSearch(searchUrl) {
  return loadIndex(searchUrl, {
    profile: 'lexical',
    maxDocuments: 10000,
    maxBytes: 4 * 1024 * 1024,
    maxQueryBytes: 1024,
    maxResults: 50,
    allowNetwork: true,
    maxResponseBytes: 4 * 1024 * 1024,
    timeoutMs: 5000,
  });
}

export function searchPublication(index, query) {
  return index.search(query, {
    limit: 10,
    fields: { title: 2, body: 1 },
    bool: 'AND',
    expand: false,
  });
}
```

Call `loadPublicationSearch` once, then pass its result and the reader's query to `searchPublication`. Each hit keeps its source record: use `hit.record.content.title` as text and `hit.record.route.url` as the navigation address. Insert titles with `textContent`, not as HTML.

Set `idleTimeoutMs` when a progressing download needs a longer overall `timeoutMs`. The idle deadline renews as response bytes arrive. The overall deadline and byte limits still apply.

If your application already has the complete records, `createIndex(records, options)` builds the same index without a request.

Every index bound and the tokenizer profile are required. `lexical` retains the provider's whitespace/hyphen tokenization and case normalization but resets its English trimming, stop-word and stemming pipeline. `english` explicitly selects that existing pipeline for the corpus; neither profile infers a document's declared language or provides universal linguistic segmentation. Index construction validates the complete dataset before calling the retained Elasticlunr provider. Duplicate content IDs or serving URLs, missing structural fields, unsafe URLs, cycles, getters, non-JSON data and exhausted limits reject the whole index. Metadata is retained as an immutable JSON snapshot; later caller mutation cannot alter destinations, snippets or ranking. Gazette-generated data is already JSON; this is not an arbitrary JavaScript object execution interface.

Ranking supports `title`, `description`, `keywords`, `route`, `body`, `notes`, `summary` and `projects`. An emitted record's `indexed` values determine which of those fields contain searchable text; unselected fields stay empty. Direct `createIndex` callers can omit `indexed`: that path uses `content.title`, `body`, optional plain `summary`, `content.keywords` and `projects[].project.id/name`, leaving `description`, `route` and `notes` empty.

Each score accompanies the complete original record. Ranking does not rewrite its relationships, language, provenance or canonical address. Missing optional text becomes an empty ranking field, not a new metadata value. Queries default to all eight fields at equal weight, AND matching and no expansion. AND requires every processed term somewhere in the selected fields, so a title match and a body match can satisfy one query. OR accepts any matching term. Explicit field boosts must be positive and at most 100. When `expand` is enabled with the English profile, literal token prefixes remain searchable alongside stemmed matches. The SDK creates an additional provider index from the same bounded, selected text on the first expanded query; this prevents a growing prefix from losing its match when it becomes longer than the stored stem. Duplicate matches keep the greater provider score, without adding the scores twice. With expansion disabled, only the existing profile matching runs. A full title match, ignoring case and whitespace, precedes other results when the title is both indexed and selected. Partial titles receive no such preference. The provider score remains available on each hit; within the same full-title preference, score ranks first and serving-URL code-unit order breaks ties. `limit` is required and bounded by `maxResults`. Empty queries and empty corpora yield an explicit empty result. There is no HTML generation, caller-defined tokenizer execution, hidden query expansion or UI state.

`index.browse(query, options)` uses the same retained index and ranking fields for a fuller discovery page. It filters the complete matching corpus before counting, sorting or pagination; it never filters the first bounded command-search hits. An empty browse query selects the captured corpus, while `search` keeps its empty query result. `limit` remains required and bounded by `maxResults`; `offset` is a nonnegative integer bounded by `maxDocuments`. `sort` is `relevance` (default), `latest` or `oldest`. Date sorts use only the supplied publication date, put undated records last in both directions and break ties by serving address. Dates and timestamps share the native reading index’s publication-day order: date-only values use their authored day and timestamps use their UTC day. Within each day, timestamps compare their actual instants in the selected direction, followed by entries whose time is unknown in both directions. This display convention assigns no time to a date-only value. Timestamps retain their supplied fractional precision and timezone instant.

```javascript
export function nextResearchPage(index, query) {
  return index.browse(query, {
    limit: 15, offset: 15, sort: 'latest',
    filters: { kind: ['article'], project: ['project:example'] },
  });
}
// The returned page has total and facets for the complete matching population,
// plus hits for this page of results.
```

Filters accept exact native `site`, `kind`, `author` or `project` identity arrays, exact authored `keyword` phrase arrays, or `classification: [{ taxonomy, term }]` pairs. Values within a dimension are OR; dimensions are AND. Omit inactive dimensions; supplied arrays contain 1–32 unique values, with identity strings no longer than 512 characters. Author membership uses explicit resolved author attributions, never site defaults or rights-holder roles. Classifications remain scoped by their taxonomy: equal term keys do not merge. No route, keyword or display label becomes a classification. Keyword membership uses `content.keywords`, independently of the configured ranking fields: an unselected keyword can still be a discovery filter without making its text searchable. Phrases retain their exact case, internal spacing and Unicode; there is no trimming, case folding, stemming or slug normalization. Native authoring rejects empty, edge-whitespace, control-bearing and exact-duplicate phrases. The SDK deduplicates each record’s exact repeated value for facet counts. A keyword’s facet `value` and `label` are the same authored phrase; no description or taxonomy is invented. A Theme may round-trip that phrase through a `keyword` URL parameter separately from its free-text query; the SDK owns no URL parsing. The existing 32-value and 512-character filter bounds apply to this dimension too.

Facets return exact identities with per-record counts over the same complete filtered population; missing metadata contributes no value. Facets retain an optional supplied native label only when that identity has one unambiguous label across the population. Classification facets retain taxonomy and term labels independently. Missing or conflicting labels remain absent; the SDK does not invent them or supply controls.

`loadIndex` requires explicit network permission, response-byte and time bounds. The exported `MAX_SEARCH_BYTES` is the existing 64 MiB hard ceiling shared by response acquisition and retained JSON capture. A consumer may select that ceiling or a lower budget; `maxBytes` includes the complete result metadata and JSON framing, independently of the fields selected for ranking. It fetches the selected HTTP(S) URL once per load, without credentials, redirects or referrer. Fetch uses `cache: 'no-cache'`: the browser may retain a cacheable response but must revalidate it before reuse. A successful server 304 revalidation supplies the cached body as a complete 200 response to the SDK; a bare 304 without that body is rejected. Response `Cache-Control: no-store` remains effective. Only a complete 200 `application/json` response with strict UTF-8 and valid JSON is accepted. Response bytes are bounded while streaming, including when Content-Length is absent or misleading. It does not guess `/search.json`, fetch linked sites, supply a stale or local fallback on failure, or retain a partial index. The network timeout is an AbortController deadline; parsing/indexing are synchronous bounded-data work, not a worker, browser-service-worker bypass or hard CPU sandbox. Limits cap documents (100,000), encoded JSON (64 MiB), JSON nesting (64), query bytes (65,536), returned results (1,000), response bytes (64 MiB) and network time (300,000 ms). Provider working allocations are additional bounded-input overhead, not a promised fixed memory or query-time budget. Load uses native JSON parsing; it is not a duplicate-key-preserving source-document parser.

The browser cache qualification uses an owned loopback HTTP server and a fresh Chromium context, without request routing or a mocked fetch. Set `GAZETTE_TEST_PLAYWRIGHT` to the absolute path of an already installed Playwright ESM entrypoint, then run `node --test sdk/js/tests/search-cache.browser.mjs` from the repository root. An optional `GAZETTE_TEST_CHROMIUM` selects an installed browser executable. The test checks conditional 304 reuse, changed bodies, response no-store and failed revalidation; it installs no tools. Ordinary Node search checks remain in `sdk/js/tests/search.mjs`.

Errors expose only `SearchError.code`: `search_options`, `search_profile`, `search_limit`, `search_data`, `search_duplicate`, `search_url`, `search_provider`, `search_permission`, `search_response`, `search_network` or `search_timeout`. No raw response body, source text or native path appears in these errors.

Configure the module through the workspace’s BrowserModule capability and provide the search controls in your theme. Emitting search data does not add either one automatically. Preserve the embedded MIT licence notice when distributing a search bundle.

## Activate analytics after consent

Create one controller when the Theme receives the selected analytics capability. The `binding` below is its frozen public value, containing `measurement_id`. Supply `publicPageUrl` from the resolved public canonical route and `coordinates` from admitted site/content identities. None of these values comes from a query string, form field or the browser's current location.

```javascript
import { createAnalytics } from '@rtepl/gazette/analytics';

export function connectAnalytics({ binding, publicPageUrl, coordinates }, browser) {
  const controller = createAnalytics({
    measurementId: binding.measurement_id,
    publicPageUrl,
    maxEvents: 100,
    loadTimeoutMs: 10000,
  }, browser);

  return {
    status: () => controller.status(),
    // Call only from the Theme's explicit consent decision.
    async consentGranted() {
      const status = await controller.grant();
      if (status.tag !== 'loaded') return { submitted: false, reason: status.tag };
      return controller.event('content_view', coordinates);
    },
    consentWithdrawn: () => controller.withdraw(),
  };
}
```

Creation leaves consent withheld and loads no tag. Await `consentGranted()` when consent is actually granted; a successful submission means the module submitted the command, not that Google received it. Failed loading is visible in `status()`; events attempted before readiness are not queued. Wire other semantic events only where they occur, using the vocabulary in the [analytics declaration](../analytics/index.d.ts).

Withdrawal stops future submissions and is terminal for that document. Calling `grant()` again requires a new document; creating another controller in the same one is not a reset. The Theme owns its consent controls and persistence. The [property configuration requirements](../../../docs/browser-capabilities.md#analytics-waits-for-an-explicit-grant) explain automatic measurement settings and the limits of withdrawal.

## Register public reader tools

Pass the selected WebMCP binding, an already admitted public page projection and the search index created above. The adapter below supplies a callback only for a selected tool. All seven limits are explicit; choose bounds appropriate to the size of that public projection and corpus.

```javascript
import { registerReadOnlyTools, WebMcpError } from '@rtepl/gazette/webmcp';

export async function connectReaderTools(document, { binding, publicPage, index }) {
  const configuration = {
    tools: binding.tools,
    limits: {
      maxInputBytes: 4096,
      maxOutputBytes: 1024 * 1024,
      maxResults: 20,
      maxNodes: 10000,
      maxDepth: 32,
      maxConcurrent: 2,
      timeoutMs: 5000,
    },
  };
  if (binding.tools.includes('current_page')) {
    configuration.currentPage = ({ signal }) => {
      signal.throwIfAborted();
      return publicPage;
    };
  }
  if (binding.tools.includes('search')) {
    configuration.search = (query, { limit, signal }) => {
      signal.throwIfAborted();
      return index.search(query, { limit });
    };
  }
  try {
    return await registerReadOnlyTools(document, configuration);
  } catch (error) {
    if (error instanceof WebMcpError && error.code === 'webmcp_unsupported') {
      return undefined;
    }
    throw error;
  }
}
```

Keep the returned registration while the page owns those tools. Call `registration?.close()` when disposing that page's integration; it unregisters the owned tools and signals active callbacks. The search index's `maxResults` must allow the WebMCP result limit. Returning `undefined` means this browser lacks the required API: retain the ordinary page and search UI. Other failures need handling by the caller rather than silently being labelled unsupported.

The module validates and snapshots results; oversized or non-JSON values fail rather than being shortened. An asynchronous callback should honor its abort signal. Closing or timing out signals cancellation but cannot force arbitrary callback code to stop. These tools expose selected public reading data, not local files or Gazette's native MCP operations. The [WebMCP declaration](../webmcp/index.d.ts) is the exact callback/registration reference, and [capability configuration](../../../docs/browser-capabilities.md#webmcp-registers-only-selected-read-only-tools) owns the native selection and browser-support requirements.
