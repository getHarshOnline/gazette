# Local publication preview

Embed a local publication server, supervise it as a process or serve files your application has already rendered. Gazette reports which addresses are listening and lets your host stop them. In watch mode, readers continue to get the last complete build while the next one runs.

For the author’s setup, see [Your first publication](https://rawthoughts.in/gazette/first-publication/#start-the-publication). Preview serves from memory and leaves the publication’s output directory and release journal untouched. Configured generation programs need execution permission separately from reading files and opening listeners. Documentation generation also needs an explicit private staging parent, as described below. Review the [trusted-code model](architecture.md#trusted-code-and-saved-results) before granting that permission.

## Choose an entrypoint and URL mode

| Your host needs | Use |
| --- | --- |
| Serve until the host requests stop, with callbacks for state changes | `preview::runner::run`, demonstrated below |
| Keep serving while the host handles other work | `preview::session::Session`; [background ownership](#shared-runner-and-background-ownership) |
| Serve output the host has already acquired and rendered | `preview::bind`, or reserve/render/attach for local URLs; [borrowed lifecycle](#shared-operation-and-lifecycle) |
| Supervise Gazette as a subprocess | `gazette preview-sites`; [process input and shutdown](#direct-command) |

Choose the URL mode independently of watch mode:

::::tabs

:::tab "Local links"

Set `local_urls: true` in runner options, or pass `--local-urls` to the CLI. Gazette reserves the loopback sockets first, then renders links with those actual addresses. Aliases, search, feeds, sitemaps, structured data and resources use the same route model. Object and feed identities do not change.

:::

:::tab "Public links"

Keep the configured canonical addresses, redirects and cross-site links. This is useful when checking how output will link after delivery. Following a link or loading an absolute resource can contact a public host.

:::

::::

Both modes keep the publication's deployment path prefix. Publications outside the preview keep their public addresses. Literal external URLs in program bytes are not rewritten, and the local server does not restrict your browser's network access.

## Embed the shared runner

Your application supplies a parsed `build::LocalPublicationRecipe`, an exact site-to-`SocketAddr` map and a way to request stop. The recipe is the [`gazette-local-publication-set/1` input](local-rendering.md#local-site-sets), with the selected workspace and sites. Parse its bytes with `build::LocalPublicationRecipe::parse_json`, supplying the request-byte and site-count bounds. Configuration and content must already satisfy the normal rendering prerequisites; the helper does not create a publication.

This complete helper uses pinned acquisition and developer-local URLs. Pass one loopback address for every selected site; port zero asks the OS to allocate a port. The caller's status writer is private because runtime observations include input and output identities. Its stop predicate can read a host-owned cancellation flag.

```rust
use std::{collections::BTreeMap, io::Write, net::SocketAddr};
use gazette_sdk::{build::LocalPublicationRecipe, preview, workspace::Key};
use preview::{runner, watch};

pub fn serve_preview(
    recipe: &LocalPublicationRecipe,
    listeners: BTreeMap<Key, SocketAddr>,
    status: &mut impl Write,
    mut should_stop: impl FnMut() -> bool,
) -> Result<(), runner::Error> {
    let options = runner::Options {
        local_urls: true,
        listeners: preview::Options {
            allow_listeners: true,
            ..Default::default()
        },
        ..Default::default()
    };

    runner::run(
        recipe,
        listeners,
        options,
        |event| {
            if let runner::Event::Ready { state } = &event {
                for (site, bound) in &state.sites {
                    writeln!(status, "{site}: {}", bound.url)?;
                }
            }
            writeln!(status, "{event:?}")
        },
        || Ok(if should_stop() {
            watch::Control::Stop
        } else {
            watch::Control::Continue
        }),
    )
}
```

Use the reported URLs only after `Ready`: successful entry into the operation is not listener readiness. All selected listeners and the initial complete generation must be available before that event. The runner serves while it checks the stop predicate at native checkpoints; a stop request does not interrupt in-progress capture, compilation or HTTP work. When the runner returns successfully, its listeners have been released. A source, rendering, listener or status-delivery failure returns an error instead of a smaller successful preview set.

The defaults keep acquisition pinned. To adopt working-tree changes under the same bindings, select `runner::Mode::Development { interval }`; the [watch contract](#development-watch-and-rebuild) explains stale output and recovery.

## Shared runner and background ownership

Use `preview::session::Session` when your host needs to keep doing other work while serving the preview. One worker owns its recipe, captured sources, rendered files and local sockets.

::::steps

:::step "Start the session"

Call `Session::start(recipe, addresses, options, limits)`. The result is a running worker, not a ready server. A startup failure is reported separately.

:::

:::step "Wait for readiness"

Use `inspect()` for the current `Snapshot`, or `poll(after, maximum)` for events. Start the event cursor at zero and save the returned `next` cursor. Wait for `Event::Ready` before opening the reported URLs.

Polling does not drive the server. You can repeat a cursor after losing a response. If old events have been overwritten, the result reports a gap while keeping the latest state and terminal outcome.

:::

:::step "Stop and join the worker"

Call `request_stop()`, wait until `Snapshot.finished`, then call `try_finish()` to obtain the terminal result. It returns `None` while the worker is active. A `Closed` callback alone does not prove that the worker has finished.

:::

::::

Dropping the session requests a stop but does not wait for it. Use the explicit sequence when your host needs confirmation that serving ended. The [MCP preview operations](mcp.md#preview-lifetime) use this same lifetime.

## Direct command

Use `--workspace-root /absolute/path/to/workspace` to load the configured build directly, or pass one compact `gazette-local-publication-set/1` JSON request as the first line on stdin. Use the same [recipe as `render-sites`](local-rendering.md#local-site-sets). Escape the TOML newlines inside a supplied `source` value. For a workspace on disk, name its absolute root and root-relative manifest. The CLI checks the line's byte count before parsing or reading sources.

```sh
gazette preview-sites --local --allow-listeners \
  --listen site:one=127.0.0.1:0 --local-urls
```

The workspace supplies the corpus limit and math selection through `[build]`. `--max-corpus-bytes` and `--math true|false` override them; `--allow-execute` separately permits configured math and share-card generation. A share-card recipe requires explicitly pinned Node and browser bindings; no browser is selected implicitly. Each watch generation uses the settings resolved at startup. Repeat `--listen` for each site. Port zero asks the OS to choose the port. With `--local-urls`, the build uses those assigned addresses, including during share-card capture. Otherwise links use the public configuration. Read the actual URL mode and addresses in the ready result.

Add `--plan` to see which sites, loopback endpoints and effects the request calls for before it runs. The plan contains no local paths. `source_reads` is `once` in pinned mode and `repeated` in watch mode. Other fields distinguish derived-state and output writes, listeners, loopback/outbound network, Git reads and writes, release approval consumption and configured programs. The SDK checks the same plan during execution. Plan mode does not read configured sources, start programs, listen, write or contact a remote.

Keep stdin open after the initial request, or from startup when using `--workspace-root`. Any additional byte or EOF asks the server to stop. A closed input file causes immediate shutdown after startup. Wait for readiness before using the addresses and for the close result to confirm the listeners were released. If the CLI cannot write its result, it fails. See `--help` for input, rendering, header and deadline limits.

The command makes these effects explicit before execution:

| Effect | Behavior |
| --- | --- |
| Source reads | Exact configured source acquisition; `--watch` repeatedly captures disclosed working bytes while verifying unchanged Git baseline bindings |
| In-memory compilation | Shared content, Theme, resource and machine-output pipeline |
| Local network | Explicit loopback listeners and responses to connecting clients |
| Gazette output stages and journal writes | None |
| Gazette outbound platform/integration calls | None |
| Configured math/diagram programs | Only when separately selected and permitted; their effects are not sandboxed |
| Git mutation, signing, release consumption or deployment | None |

`gazette production-preview` is a narrower process entrypoint: it requires `--registry unconfigured` and `--artifact fresh-render`, fixes pinned capture and configured public links, and uses the same recipe and runtime. It does not accept local URLs, watch mode, registry reconciliation or retained prepared artifacts. It performs the reads, compilation and listening described above, including any separately permitted math or diagram generation. Gazette does not stage output, mutate Git, prepare a release, consume release authority or deploy from this command.

## Shared operation and lifecycle

Rust callers first use `build::acquire_publications` and `build::render_publications`, then pass the retained result, an exact site-to- socket map and explicit `preview::Options` to `preview::bind`. Import `preview::PreviewSession` for the session operations.

For developer-local URLs, use `preview::reserve` after acquisition, then `preview::render` with that reservation and the normal render options, and finally `preview::attach`. Import `preview::PreviewReservation` to inspect the held addresses and typed local bases. Reservation has no serving API: the same sockets remain owned throughout compilation and attachment, including OS-assigned ports. There is no probe/rebind window. Failed rendering or attachment requires dropping the reservation; the CLI does this automatically without announcing readiness.

The pure multi-site render options expose the same explicit `preview_urls` map; it must exactly match the selected sites, use literal HTTP loopback addresses with nonzero effective ports, and preserve configured path prefixes. This is projection input, not proof that a listener is held. The SDK preview path supplies the map from its reservation; attachment checks exact equality. Ordinary `bind` refuses developer-local output because it cannot reacquire the original session.

The map belongs to this session's private evidence and sockets. It never enters the workspace or SQLite registry, so another preview or a later production render cannot inherit it, including after cancellation, panic or process exit. Durable staging preparation refuses developer-local output before any writer, journal or filesystem action.

- Every requested render must have succeeded. Binding a smaller successful subset is refused, including when only one site failed.
- Every site has one explicit loopback address. Port zero requests an OS-assigned port; the session's `addresses()` reports the actual bound set and `urls()` projects usable local entry URLs with their configured path prefixes.
- All listeners are held before readiness. Failure on a later bind releases earlier sockets without claiming a reduced ready set.
- `poll()` accepts at most one connection and performs bounded request/response work. An idle result permits a short caller wait. Client disconnection or timeout is distinct from response completion; listener failure terminates the set instead of silently losing a site.
- For a directly borrowed HTTP session, `close()` or drop releases its sockets. This differs from the [background handle](#shared-runner-and-background-ownership), whose drop requests stop rather than proving closure.

The output is immutable for each borrowed generation lifetime. Source or Theme changes after acquisition do not change those bytes. A fresh input capture and complete render are needed to install a different generation.

## Development watch and rebuild

Select `runner::Mode::Development { interval }` in Rust or `--watch` in the CLI to recapture working-tree changes. Either URL mode is supported. The initial workspace, site recipes, source roots, commits, parent gitlinks, Theme selections and listener addresses stay fixed; changing those bindings requires a new run. The CLI retains the configuration resolved from its initial request or `--workspace-root`; configuration changes require a restart.

The configured checkout baseline must still exist and match the declared HEAD and parent/child pins, while working files beneath the selected roots may differ. These captures are development observations, including when their bytes match the baseline. They cannot enter verified-source materialization or durable publication preparation. Pinned acquisition instead requires byte equality with the baseline.

Each `--watch-interval-ms` interval (default 500, allowed 100–3600000) performs a fresh confined capture of the exact configured source universe, including the shared Theme, explicitly enabled plugin packages and the content needed for cross-site relationships. Capture and Git metadata remain subject to the normal per-source and aggregate ceilings. Polling compares retained tree identities, not mtimes or event labels: same-byte saves and bursts coalesce, while additions, deletions, renames and atomic saves become actual capture changes. No machine-wide scan or native watcher installation is involved. Changes during capture remain subject to the reader's consistency checks; this is not an atomic cross-repository snapshot.

Changed inputs run through the same complete semantic, Theme, resource and machine output compiler. The SDK keeps one bounded resource cache for the watch session, so unchanged resource requests can reuse compiled bytes after fresh acquisition. The exact retained input tree (including unused declared prerequisites), recipe, selected module URLs and every effective compilation limit form the key. Compiler- adapter replacement starts a new cache. No error is cached; every reused artifact still passes the application's dependency, geometry, reference and output checks. Every requested site must succeed before replacing the generation. Listener reservations stay held during acquisition and compilation. A scoped transport worker serves the previous complete generation while that bounded work runs; compilers and their session-local caches remain on the calling thread. The worker is joined before the complete output set changes, including on failure or unwind. Per-request transport limits still apply. This does not provide per-browser multi-request snapshot isolation: requests on either side of a successful replacement can observe different generations.

The private control stream reports:

- `ready`: initial complete generation attached to actual listeners.
- `changed`: a new complete generation installed on the same sockets (the Rust observer's `Event::Rebuilt`).
- `rebuild_failed`: the old generation remains served but is explicitly stale.
- `recovered`: fresh acquisition again matches the previously retained generation; its generation number does not falsely advance.
- `closed`: the session's listeners are released.

Observer/report-delivery or listener failure terminates the run; a later build failure instead keeps the last complete generation and reports the reason. Source and Theme changes that arrive during a build are acquired on a subsequent poll. No browser auto-refresh script is injected; refresh the browser after a successful rebuild. Live configuration rebinding and browser live reload remain separate work.

For direct watch-loop integration, `preview::watch::run` accepts its fixed `Request`, bounded `Options` and an observer returning `Control::Continue` or `Control::Stop`. Its `Event::Poll` is a quiet cancellation checkpoint, not a progress notification. This application-owned loop uses the normal build adapters and retains each generation's exact source and render evidence without copying payloads or self-referential borrows.

## Include generated documentation in discovery

Imported reference pages join search and reading outputs without running a generator. For a configured generated reference, `preview-sites --documentation-generation-parent /absolute/private/stages --allow-execute` selects an existing staging parent and permits its pinned tools to run. The parent must be separate from all captured source and configuration roots. The [rendering guide](render-outputs.md#documentation-reading-pages) explains the complete source limits, including an explicit entry ceiling for builds that need more than the default.

The CLI emits original documentation results before `Ready`, and the served state retains the same results. Failed rendering or output delivery does not erase stages already created; inspect the recorded paths before retrying. Documentation results have their own bounded capacity, separate from the preview state’s metadata limit. A lost output pipe returns failure and preserves the complete evidence on stderr.

SDK hosts selecting generation use `preview::runner::run_with_documentation_delivery` and consume its `DocumentationDelivery` callback, including original native failure custody, before returning from that callback. The ordinary `runner::run` entry refuses this selection. Background `preview::session::Session` also refuses it before creating a worker, with `preview_documentation_custody_unavailable`; use the synchronous callback entry for this case. In watch mode, unchanged captured documentation inputs reuse their generated catalog. Changed inputs regenerate it; an unchanged failed attempt retains and replays its original failure instead of running the tools again.

## Developer-local documentation links

Run Gazette's publication preview and Mintlify's documentation preview as two separate servers. Each reports its own local address. To make links from the publication open that local documentation server, a publication recipe can supply `preview_link_bases`, a private JSON object mapping exact public bases to literal HTTP loopback bases with the same mount path. Use the documentation server's actual held address, for example:

```json
{"https://docs.example/": "http://127.0.0.1:43000/"}
```

The publication operation requires developer-local `preview_urls`; the preview runner derives those site URLs from its reserved listeners. Ordinary hyperlinks to those selected publications use the same local bases as identity references. The additional map projects documentation hyperlinks and configured Docs resource actions at the URL-binding boundary, preserving the remaining path, query and fragment. An owned documentation destination without a selected local provider refuses the local render instead of silently linking to its public host. Unrelated external addresses, canonical/source identities, code examples and raw text stay unchanged; this requires no new authoring syntax.

The CLI can add this mapping without editing the saved request:

```sh
cat "$GAZETTE_REQUEST" - | "$GAZETTE" preview-sites \
  --local --allow-listeners --local-urls --watch \
  --listen site:field-notes=127.0.0.1:0 \
  --preview-link-bases-json '{"https://docs.field-notes.example/":"http://127.0.0.1:3909/"}'
```

Use the documentation server's actual address. The flag requires `--local-urls`. It adds the map to the native preview request; it does not change the saved request or article. Conflicting maps, duplicate keys and non-loopback destinations fail before serving.

For the reciprocal direction, the [Mintlify target](mintlify.md#local-preview-links) accepts the same native map while assembling a private preview. Use the actual Gazette listener address for that return link. Both outputs retain the selected map in their private evidence. No override is written into the workspace or registry, and durable preparation or maintained-output replacement refuses an output carrying this local projection. A fresh ordinary assembly/render without the map retains public links and refuses loopback hyperlinks in page and manual prose; code examples remain literal.

## Requests the preview server accepts {#http-boundary}

The HTTP adapter uses a pinned native request parser and the existing `RoutePath` codec to map validated paths to exact retained files. Directory indexes require an actual retained `index.html`; missing files do not fall back to another page or native directory. Request handling cannot open symlinks, special files or changed disk entries.

### Requests and connection lifetime

Requests require HTTP/1.1, GET or HEAD and exactly one Host matching the actual socket address reported at startup. DNS names are not accepted as equivalent hosts. The profile rejects request bodies, transfer encodings, upgrades and unsupported request forms. It closes each connection after one response and does not process pipelined follow-up requests. It does not promise to detect bytes that arrive after that response has begun.

A valid local page navigation with no retained output returns an HTML 404. It links to Home at the selected mount when present, and to Search when the selected search page was emitted. HEAD sends the same headers without a body. Missing resources and refused requests keep the plain response. The recovery page is assembled when the preview attaches a complete output.

### Response compression

Preview negotiates gzip through `Accept-Encoding` for text, JSON, XML, feeds, manifests and SVG. It compresses bodies of at least 1 KiB when doing so saves bytes; when the client explicitly refuses identity encoding, smaller text can also use gzip. Quality values, wildcard preferences and explicit refusals are respected. If neither available representation is acceptable, preview returns an empty 406 response. Malformed encoding preferences return 400.

GET and HEAD use the same negotiated `Content-Length` and `Content-Encoding`; HEAD sends no body. Responses carry `Vary: Accept-Encoding` and retain `Cache-Control: no-store`, so a changed preview never depends on an older browser cache. Encoding happens within the response deadline with a bounded temporary buffer; it changes neither the retained source bytes nor their identity. Images, fonts and video remain unencoded, and video range offsets still refer to their original bytes.

This describes the local preview. A deployment host must negotiate its own compression and caching; Gazette does not establish those settings by emitting a compressed sidecar file.

### Response formats and media ranges

Header byte/count ceilings apply before unbounded retention. Request and response timeouts are total deadlines for each phase; trickling bytes does not renew them. Generated pages, resources, feeds and machine outputs retain their producer-declared media types even when their configured paths have no extension or a misleading suffix. The transport does not sniff their bytes or reconstruct formats from filenames. Untyped static assets use a bounded filename mapping; unknown types remain non-sniffable binary data. Retained `.mp4` and `.webm` files use `video/mp4` and `video/webm` respectively, and advertise `Accept-Ranges: bytes`. GET supports one closed, open-ended or suffix byte range, slicing borrowed bytes without copying. A valid unsatisfiable range returns 416 and `Content-Range: bytes */length`; malformed, unknown-unit or multipart ranges return the complete representation. HEAD ignores Range; If-Range receives the full representation because preview has no validators. Duplicate Range headers and ranges on other media remain refused. Range handling does not validate the media container. Responses are non-cacheable and supply no permissive CORS policy. The local server provides neither authentication nor process isolation.

## What preview can and cannot do {#boundaries}

Preview success establishes local serving of the selected retained generation. Release, disclosure checks and browser interaction assessment have their own owners; preview does not replace them. Render and acquisition bounds still apply alongside the HTTP limits above. Use only content and scripts trusted for execution in the selected local browser.

The [SDK runner](../sdk/rust/src/preview/runner.rs), [background session](../sdk/rust/src/preview/session.rs) and [borrowed session](../sdk/rust/src/preview.rs) expose the implementation entrypoints. The [MCP contract](mcp.md#preview-lifetime) defines its process-local handle and result-delivery rules over the same runtime.

When changing preview behavior, check the SDK session lifecycle, MCP handle lifecycle and CLI server behavior:

```sh
cargo test --locked -p gazette-sdk --test preview
cargo test --locked -p gazette-mcp --lib preview
cargo test --locked -p gazette-launcher --test preview_mcp
cargo test --locked -p gazette-launcher --test publication_render preview
```
