# Add analytics and browser tools {#browser-capabilities-use-frozen-public-inputs}

Add analytics or browser WebMCP to a Theme by declaring the client, its settings and its browser files in the workspace. Gazette checks those inputs before rendering and passes their URLs and settings to the Theme. Analytics starts only after consent. WebMCP offers read-only tools over the public data already on the page.

## Declare a client and its browser files {#configuration-selects-bindings-and-resources-separately}

Enable each client under `composition.capabilities`. You can enable either one on its own:

```toml
[composition.capabilities]
"publication.analytics/v1" = { state = "enabled", provider = { kind = "gazette", provider = "google-analytics-client" }, requirement = "required" }
"publication.webmcp/v1" = { state = "enabled", provider = { kind = "gazette", provider = "webmcp-client" }, requirement = "required" }
```

Declare public client inputs inside the relevant existing `[[brands.publications]]` table:

```toml
processing = { analytics = { enabled = true, provider = "google-analytics", measurement_id = "G-ABC123" }, webmcp = { enabled = true, tools = ["current_page", "search"] } }
machine = { search = "lexical" }
```

Use a measurement ID beginning with `G-` followed by uppercase letters or digits. The complete ID can be up to 64 ASCII characters. Give WebMCP a nonempty list of unique `current_page` and `search` tools. Gazette keeps that order. Enable publication search if you include `search`. An enabled capability needs its matching enabled processing entry. Unknown fields, missing inputs and conflicting settings fail the workspace check.

Gazette prepares these public values without network calls:

| Client | Value the Theme reads |
| --- | --- |
| Analytics | `{"contract":"publication-analytics-v1","measurement_id":"G-ABC123"}` |
| WebMCP | `{"contract":"publication-webmcp-v1","tools":["current_page","search"]}` |

Look in `presentation.capabilities` in the Theme and `capabilities` in the site model. Provider identities and revisions stay in the private report. The provider cannot change the configured ID or tool names.

Declare the JavaScript modules separately through [page resources](page-resources.md). Choosing a capability does not load a plugin, resource, script or export. Pass the values from the Theme into the module you load. The compiler and template make no live requests.

The SDK's [`browser-resources.toml`](../sdk/js/browser-resources.toml) exports `module:analytics-sdk` and `module:webmcp-sdk` from their original source files. Bind a source rooted at `sdk/js`, register that manifest as plugin package `gazette-browser-sdk`, then select the needed resource:

```toml
[[plugin_packages]]
package = "gazette-browser-sdk"
source = "source:browser-sdk"
manifest = "browser-resources.toml"

[composition.plugins]
gazette-browser-sdk = { state = "enabled" }

[composition.resources]
"module:analytics-sdk" = { state = "enabled", resource = { owner = { kind = "plugin", package = "gazette-browser-sdk" }, resource = "module:analytics-sdk" }, output = "/assets/analytics-sdk.js", use = "dependency" }
```

The captured source must include the manifest and its declared inputs: `analytics/index.mjs`, `webmcp/index.mjs` and `webmcp/data.mjs`. A Theme's consent module can bind an import to `module:analytics-sdk` without copying the SDK or requiring it in every Theme consumer. The workspace selects the resource and the matching capability independently; loading the SDK alone does not activate analytics or WebMCP.

## Analytics waits for an explicit grant

Import `createAnalytics` from the browser SDK's `@rtepl/gazette/analytics` subpath. See [Activate analytics after consent](../sdk/js/docs/browser.md#activate-analytics-after-consent) for the controller and consent flow.

::::steps

:::step "Create the controller"

Pass the configured measurement ID and the resolved public canonical URL as `publicPageUrl`. Do not pass the browser location. Gazette rejects credentials, queries and fragments. The controller sets page location and suppresses title and referrer inputs. It does not read the DOM, request a tag, access storage or instrument the page when created.

:::

:::step "Ask for consent before loading the tag"

The Theme provides the consent UI and any persistence. Call `grant()` after the reader agrees. Gazette uses Google's [basic consent model](https://developers.google.com/tag-platform/security/concepts/consent-mode), which keeps the tag unloaded before grant. A competing tag, data layer or controller causes an error.

:::

:::step "Send an event from the public page model"

`event()` accepts `content_view`, `cross_site_navigate`, `cta_view`, `cta_click`, `intake_start`, `subscribe_start` and `discussion_open`. Pass public `site` and `content` IDs and, if applicable, `journey` and `button`. They have size limits. Email addresses, labels, form values and arbitrary fields fail. Calls before grant or tag completion are neither queued nor replayed.

:::

:::step "Stop submissions if the reader withdraws"

`withdraw()` turns on Google's opt-out switch. It cannot undo a tag request or event already sent. A later grant on the same controller and document returns `analytics_new_document_required`. The Theme decides whether to take a new-document action.

:::

::::

Gazette turns off automatic pageview submission. To use only the seven documented events, the property owner must also disable Enhanced Measurement and check connected-tag settings. `send_page_view: false` does not disable every automatic event. Tag completion does not confirm that Google received an event or that the property is ready. Check privacy and publisher requirements separately.

## Register read-only browser tools {#webmcp-registers-only-selected-read-only-tools}

Import `registerReadOnlyTools(document, configuration)` from `@rtepl/gazette/webmcp`. The import registers nothing on its own. Gazette uses the browser's [document model context API](https://developer.chrome.com/docs/ai/webmcp/imperative-api). There is no connection to the stdio MCP server, localhost bridge or legacy `navigator` shim.

Pass the configured tool names, their callbacks and positive `limits` for input/output bytes, result count, JSON nodes/depth, concurrent calls and timeout. Return the page's existing public model in `currentPage`. For `search`, query the public corpus with the [browser search SDK](../sdk/js/docs/browser.md#search-the-emitted-corpus). Use those values directly instead of scraping the page, exposing private results or guessing identity from a URL. See [Register public reader tools](../sdk/js/docs/browser.md#register-public-reader-tools) for all seven limits and a working registration.

Complete site rendering exposes `presentation.webmcp` only when the capability is available and the current authored record is eligible for discovery. It contains `content` using the existing discovery metadata contract, the resolved `route` and retained `body_text`. Pass that object to `currentPage`; do not serialize the full template context. Generated listing slices, aliases, mirrors, graph and missing-page views omit it.

Keep the registration handle. `close()` removes the tools it owns and signals active callbacks. If registration fails partway, it aborts earlier registrations. Each call validates inputs and returns a complete immutable JSON value. An oversized result fails without truncation. Timeout or cancellation signals the callback, but it still counts against the concurrency limit until it settles.

Tool annotations describe read-only operations over untrusted public data. They do not grant access or make the data safe. No tool writes files, navigates, subscribes, builds or releases. Without the browser API, registration returns `webmcp_unsupported`. Keep HTML, Markdown and search links usable without WebMCP, and check support in the browser you target.

### Test WebMCP in Chrome

Select exact installed Node and Chrome executables. These opt-in tests compile the retained SDK into disposable Gazette pages, start native loopback previews, and exercise the browser's real tool discovery, execution, cancellation and registration cleanup. They also check the disabled and no-JavaScript paths. The observer waits for a bounded native result, not a fixed page-load delay.

```sh
GAZETTE_TEST_NODE=/absolute/path/to/node \
GAZETTE_TEST_CHROME=/absolute/path/to/chrome \
cargo test --locked -p gazette-launcher --test publication_render \
  webmcp:: -- --ignored --test-threads=1
```

The test requires a Chrome build with the experimental WebMCP API. It enables that feature only in its disposable profile; missing or incompatible support fails the test. It does not publish a site or change your installed browser's configuration.

## Subscriptions are not implemented by these clients

Enabled subscription processing remains explicitly unsupported. Neither module creates a signup form, calls Apps Script, selects a Sheet or infers double opt-in and unsubscribe behavior. Contact links remain ordinary configured email links.

See [workspace configuration](workspace.md) and [the Theme contract](theme-reference.md) for the surrounding composition boundaries.
