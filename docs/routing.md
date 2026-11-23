# Public routes

A page’s readable `content.slug` combines with its publication’s routing rules and editorial placement to give it a public address. The content ID stays stable when the address changes. An explicit `route.path` handles an address exception; its aliases redirect released old addresses, while the output path tells Gazette where to write the file.

Set the base URL in the brand's publication binding:

```toml
[[brands]]
id = "brand:example"
name = "Example"

[[brands.publications]]
site = "site:example" # Must also exist in the workspace's sites table.
public_base_url = "https://example.test/publication/"
```

Gazette reads the public base here. A brand name, site key, repository remote, source folder or `documentation_repository` path cannot supply it. The developer documentation output has its own destination.

The resolved route keeps three address roles: `url` is the record’s serving address; `canonical_url` is its original’s serving address and may be local in an explicit preview; `public_url` is the original’s canonical public address from the immutable publication configuration. Public linked-data identities use `public_url`, while navigation can remain within the selected preview.

## Routes from editorial placement

Opt in on the publication binding and name its home by exact content ID:

```toml
home = "content:home"
routing = { articles = "/blog/", collections = "/collections/", projects = "/", pages = "/" }
```

Each prefix must be a directory route. `articles` serves standalone article pages; `collections` serves root editorial collections; `projects` serves project pages and root documentation collections; `pages` serves other standalone pages. Configure the categories the publication uses. A missing rule fails when a root needs it. The configured home derives `/` without a slug; its page family remains as authored.

In a page or container’s existing `[content]` table, declare its own readable component:

```toml
slug = "first-step"
```

A slug is one URL component with the same Unicode, escaping and confinement rules as a full route. It is independent of the content ID, display title, source folder and project name. Gazette does not invent a slug. Once routing is enabled, an unrouted non-home node must provide one.

Every contained node appends its slug to its immediate editorial parent’s resolved directory route. A collection follows its parent collection; a journey follows its containing collection; a leaf follows its sole collection or journey. Thus a root `systems`, nested collection `practice`, journey `walking` and leaf `first-step` produce `/collections/systems/practice/walking/first-step/`. Changing the root slug changes all derived descendant addresses while retaining their own slugs and stable IDs. Exact identity links follow the new addresses without rewriting their visible labels.

Containment can cross configured sources bound to the same site: a publication collection can contain a product journey while each body and its media stay in their original bundle. Source paths, About/Mentions/Context edges and browsing links do not determine routes. Invalid or cross-site containment fails before derivation. A file route cannot serve as a derived child’s parent.

A root documentation collection with slug `gadget` serves its introduction at `/gadget/`, with a child `getting-started` at `/gadget/getting-started/`. Project facts join after these routes exist. Developer documentation keeps its separate configured destination.

Existing explicit routes take precedence and retain their canonical, alias and mirror declarations. A derived node adds no automatic aliases. All explicit and derived routes pass the same canonical, collision, base URL and output checks; generated paths also consume bounded text allowances. Derived values have no fabricated frontmatter span. Without publication routing, a missing route remains unrouted for structural inspection.

## Explicit address exceptions

Inside the existing `[content]` table, add the route:

```toml
route = { path = "/guide/", aliases = ["/old-guide.html"], mirrors = [{ site = "site:other", path = "/guide-copy/" }] }
```

| Declaration | Where the reader goes | File within that site's output |
| --- | --- | --- |
| Primary route | `https://example.test/publication/guide/` | `guide/index.html` |
| Alias | `https://example.test/publication/old-guide.html` | `old-guide.html` |
| Mirror | The other site's configured base plus `/guide-copy/` | `guide-copy/index.html` in the other site's output |

A deployment prefix remains in URLs and does not appear again beneath the output root. `/` maps to `index.html`. Use a trailing slash for a directory index. A path without it names a file, even without an extension. Gazette never falls back to a slug from a title or filename.

Use `aliases` for old addresses on the same site and `mirrors` for a redirect on another configured site. Both point directly at the canonical URL. A mirror does not move the content, copy the body or grant republishing rights. Missing sites or base URLs, duplicates and collisions fail. A mirror may point at a page whose repository and project placements have separate declarations.

Rendering a mirror needs a configured alias template. Its `MirrorModel` has `content`, `canonical_site`, serving `site` and `url`, and `route.canonical_url`. The mirror also gets the serving site's `presentation`: Theme, palette, capability outcomes, compiled resources/modules and interaction/search bindings. Gazette uses their configuration rather than deriving those choices from the Theme's identity. Same-site aliases get the full page model. `model.json` stores mirror values under `mirrors`, keyed by output path.

A site can render only mirrors. Search, feeds, sitemap, JSON-LD and `llms.txt` do not list those redirects as pages. Excluding the canonical object also excludes its mirrors. Check the deployed redirect separately from the local render.

## Syndicated bodies

If you author a copy on another publication, declare the original by content ID:

```toml
route = { path = "/republished-guide/", canonical = "content:original-guide" }
```

The original must be in the resolved content set, have its own route and name no other canonical target. Missing originals, self-reference and chains (including cycles) fail. A build cannot include a copy and exclude its original. The declaration records the relationship without copying the body, comparing the text or deciding rights.

| Value | Where Gazette gets it |
| --- | --- |
| `url`, output path, relative body links and source | The copy |
| `canonical_url` and the canonical object reference | The original |
| Publisher, author, rights holder and lifecycle | The copy's own declarations |

Page metadata and graph records keep the canonical object reference. Article JSON-LD names the original without assigning the copy's breadcrumbs or publisher to it. Search keys and feed links use serving addresses. Sitemaps include originals and omit copies and redirects. A mirror declared on a copy still leads to the original.

## Address validation

Gazette encodes safe percent escapes and Unicode consistently in URLs and uses decoded UTF-8 for output names. Case and Unicode normalization stay as written. The schema rejects traversal, repeated or encoded separators, nested escapes, controls, queries and fragments. An HTTP(S) base needs a directory prefix and cannot contain credentials, a query or fragment. The `url` library normalizes hosts, schemes and default ports, and gives a host without a path `/`. Gazette does not test whether a provider serves the address.

Gazette checks all declared routes together, including draft and archived objects and container landings. Self-aliases, duplicate aliases, overlapping primary and alias routes, `/guide/` versus `/guide/index.html`, file/directory conflicts and shared public addresses fail. Two sites can use the same relative path when the addresses differ. The [output writer](output-materialization.md) separately checks for collisions with emitted files and filesystem case or Unicode aliases.

`ResolvedContent::route` owns the compiled value. `PageProjection::route` borrows it, and `CompiledTheme::render_page` passes it through the shared model. Consume that value instead of reconstructing the canonical URL. Gazette normalizes alias order and keeps source spans and digests. Expanded URLs have their own text limit using the content limit, so repeating a long base across many objects still uses that allowance.

A [preview map](preview.md) supplies session-only bases through `ResolvedContent::base_url`. The route compiler uses them for local addresses without changing the workspace or stable IDs. Templates, assets, resources and feeds use the same bases. Sites outside the preview set keep their original locations. The renderer does not replace URLs independently.

An explicit or derived route needs a configured base. HTML, JSON-LD, graph, navigation, search, feed, sitemap and `llms.txt` use these compiled routes. Rendering the redirect, writing files and approving release are separate operations. Use the dedicated contracts for external canonical overrides, relocations and symbolic cross-site lookup aliases.

## Next

[Links and crossings](links-and-crossings.md) shows how these routes become links within a publication and across publications. [Read the generated site files](render-outputs.md) lists the files a build writes at each route.
