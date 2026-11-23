# Served-output observations

Give Gazette the files you expected to deliver and the routes to check. It requests those routes from the server and reports whether each HTTP response matches the corresponding file byte for byte. It leaves the local files and release journal untouched.

Check the routes you care about after a push or platform build. The report applies to those routes at the time of the requests.

## One operation across interfaces

::::tabs

:::tab "Rust"

Use `delivery::observe` with an output capture you already have. Use `observe_retained` when the output is on disk. Supply the tree identity you stored separately. Gazette checks the entire tree against it before making any HTTP request.

:::

:::tab "CLI"

Pass one JSON request to `gazette observe-delivery` on stdin. Keep it private because it names local paths and the files you expect to serve.

:::

:::tab "MCP"

Pass the original JSON bytes in `request_json` to `observe_delivery`. Use the discovered `contentSchema` for the SDK request shape. Passing the original bytes lets Gazette detect duplicate fields. The VS Code application-tool list exposes the same tool.

:::

::::

| Request field | What to provide |
| --- | --- |
| `tree` | Absolute path to the existing output directory |
| `tree_identity` | Identity of the entire tree, stored separately from the files you are checking |
| `source_limits` | File and directory read limits |
| `curl` | Path to a trusted executable, its pin and its working directory |
| `selection.base_url` | Serving base, including the deployment prefix |
| `selection.routes` | Nonempty list of unique site-relative routes, in the order to check |
| `options.http` | Separate read, process and network permissions, response size and timeout |
| `options.max_routes` | Maximum routes to check, at most 256 |

Gazette maps a directory route to its `index.html`. It checks that every requested route maps to a regular file before sending the first request. An invalid route cancels the whole request. CLI input and result limits are separate from HTTP response limits.

## Response meaning

Each route has the requested URL, the local file path and the expected byte count and hash. An HTTP 200 response matches only if the complete body matches byte for byte. A non-200 response, including a redirect, records its status and complete body identity as a failure. A transport error, limit or partial response is reported as unavailable.

Gazette requests identity encoding and compares the bytes it gets. HTML normalization or CDN rewriting can produce a mismatch even when the page looks the same. Requests happen in order, so the report is not an atomic snapshot across routes. Observation time stays out of the deterministic build identity.

The CLI exits nonzero and MCP returns a tool error when any route fails to match. Both can return the full report when it fits the output limit. A lost or oversized result cannot undo HTTP requests that already ran.

## HTTP transport {#native-transport-boundary}

Only HTTPS and literal numeric loopback HTTP are supported. Gazette runs the pinned curl executable without inherited credentials, curl configuration, proxies, redirects, retries or a cached fallback. The process runner limits execution and captures complete stdout. Use a trusted executable under the [shared execution model](architecture.md#trusted-code-and-saved-results). Gazette does not sandbox it or verify its libraries, certificates or the destination service.

The GitHub Pages adapter shares this HTTP transport but interprets provider responses separately. Templates, rendering and compilation make no live requests. The comparison describes the routes you asked for at the time of observation. It grants no permission to publish and cannot guarantee future availability.

## Next

[Releasing](releasing.md) covers how approved output reaches its destinations, which is what you check here afterwards. [Preparing deployment files](deployment-output.md) covers the hosting files a publication emits.
