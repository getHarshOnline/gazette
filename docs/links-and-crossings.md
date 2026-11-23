# Links and crossings

A crossing exists because a thought often continues in another inquiry: a link to another configured publication lets the reader follow it there without losing the argument they came from. Use Gazette's resolved link attributes to style a link that crosses from one publication to another. Gazette resolves links for the whole configured publication set. The Theme can show a destination site's visual identity while the reader stays on the page that contains the link.

## How a link is classified

| Destination kind | What Gazette resolved |
| --- | --- |
| `internal-native` | A link on this publication, including a root-relative `/path/` |
| `cross-site` | An address on another configured publication |
| `external` | An address outside the configured publications |
| `resource` | A compiled asset or download for the site |

Gazette checks declared routes, aliases and mirrors. A crossing can name another site without matching a page there. In that case the link has the destination site's information but no destination page. Gazette never requests the URL to find the page.

## What a link carries

| Attribute | What a Theme reads |
| --- | --- |
| `data-gazette-link` | Stable ID for this occurrence of the link |
| `data-gazette-destination-kind` | One of the four destination kinds |
| `data-gazette-destination-content` | The target page's content ID, if Gazette found a page |
| `data-gazette-destination-site` | The destination site ID, if configured |
| `data-gazette-destination-relationships` | Existing `related`, `collection`, `collection-member`, `journey` or `journey-member` relationships |
| `data-gazette-destination-theme` | The destination site's Theme package |
| `data-gazette-destination-palette` | The destination site's palette, if configured |

An attribute appears only when Gazette has the value. Navigation entries, search records and graph nodes get the same destination Theme and palette. Read those fields to style the link. The [Theme reference](./theme-reference.md) describes the full presentation data.

Links do not make collections, journeys or related-page declarations. The [content model](./content-model.md) defines how those relationships are authored.

## Writing links in site content

Link to a page in the configured publication set by domain, route and optional fragment:

```markdown
Read the [[site:example.org/research/paper/#results|paper's results]].
```

The route can be a page's address or alias. Gazette stops the build when the site is missing or ambiguous, or the route or fragment is missing. A local preview uses the local serving address when available. A public build uses the public address. Use this syntax for page links only. Inside a table, follow [links in tables](markdown.md#links-in-tables) so the separator remains part of the link.

Use ordinary Markdown for external URLs, such as `[background](https://elsewhere.example/paper/)`. An absolute URL stays as written unless a [preview link map](preview.md#developer-local-documentation-links) points its base at a local server. Use public URLs or publication links rather than paths on your machine: a machine-local path exposes that location and does not give readers a working destination on the live site.

Authored content and article notes reject relative links to Markdown files, such as `../guide.md`. Encoded or differently cased `.md` and `.markdown` suffixes follow the same rule. Repository manuals and mounted native documentation keep the relative-file contract described below.

Declare lasting relationships by ID in the frontmatter arrays before their tables:

```toml
+++
related = ["content:research-paper"]
collections = ["collection:research"]
projects = [{ project = "project:engine", kind = "about" }]

[content]
id = "content:commentary"
site = "site:example"
title = "A commentary on the paper"
lifecycle = "published"

[content.structure]
kind = "page"
family = { kind = "article" }
+++
```

## Writing links in project documentation

Within a developer manual, use a relative link such as `./install.md` or `./install.md#what-a-build-needs`. Gazette rewrites it to the destination's configured page address. It stops assembly if the page or heading is missing. See [Mintlify manual inputs](mintlify-formats.md#authored-developer-manuals) for what can be a destination.

To reach product help, use its public URL. A [preview link map](mintlify.md#local-preview-links) can point it at the local publication. For an author’s example, see [Connect publications](https://rawthoughts.in/gazette/connect-publications/).

## What a crossing never does

A crossing does not copy the target page, add its site to the build or verify that its content is current, approved or reachable. Give it the same target size, focus, keyboard behavior and timing as other links. Use [routing](./routing.md) for redirects and aliases, including mirrors served on another site.

A citation download uses the same asset path and serving URL rules as a body link. The site build must include an asset referenced only in `content.citation` too. Gazette serves a `.bib` or PDF file as supplied without validating its contents. It does not fetch or verify external destinations.
