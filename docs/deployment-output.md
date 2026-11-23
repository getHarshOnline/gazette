# Preparing deployment files

Gazette prepares deployment files inside the same bounded output tree as pages, resources and discovery documents. Rendering does not deploy a site, change a domain, contact an account or approve publication. The existing local and candidate content selections still decide which pages enter that tree.

## Hosting files

Select hosting in the publication's processing configuration:

```toml
[defaults.publication.processing]
hosting = { provider = 'github-pages' }
```

The native SDK supplies the GitHub hosting file provider. Each selected publication emits an empty `.nojekyll`. A custom DNS host in its configured `public_base_url` also emits `CNAME` with that host and a final newline. A default `*.github.io` address needs no `CNAME`. Paths do not enter `CNAME`, and a loopback address cannot substitute for the production domain. Preview may replace links with listener addresses, but hosting files use the original configured production address.

Application callers can supply the pure `gazette_ports::hosting::HostFiles` service through the `*_with_host_files` rendering methods. Existing methods use no provider and reject a publication that selects hosting. Development preview selects this service through `watch::Adapters.host_files`. Provider files pass through ordinary path collision checks and output limits before a complete tree can be returned.

## Crawler policy

Crawler rules belong to the workspace or publisher, with site overrides using ordinary publication inheritance. Gazette does not infer which crawlers search, answer questions or train models.

```toml
[defaults.publication.machine]
sitemap = true
robots = { enabled = true, rules = [
  { user_agent = '*', allow = ['/'] },
  { user_agent = 'ExampleTrainingBot', disallow = ['/'] },
] }
```

The example bot name illustrates a publisher-selected rule; it does not identify a real service. `robots.txt` preserves group order and emits each group's allowed paths before its disallowed paths. Gazette appends an absolute `Sitemap` line only when that assembly actually emits a sitemap. A site's `rules` array replaces inherited rules as a whole; `enabled = false` suppresses the file while preserving the inherited configuration. Omission emits no policy. Duplicate user-agent groups, directive injection and unbounded rule sets fail validation.

These directives request crawler behavior. They do not make content private or prevent access to files.

## Terms of use

```toml
[defaults.publication.machine]
terms = { path = '/terms/', tdm_reservation = true }
```

`path` is the site-relative route of the publication's terms page, beginning with `/`. Gazette joins it to the production `public_base_url`, so preview output still names the published terms. It inherits like `robots`, and a site can override either field. Any declared `terms` adds the address to [JSON-LD](json-ld.md#projection-contract) as `usageInfo`. With `tdm_reservation = true`, Gazette also writes `.well-known/tdmrep.json` with one rule, `[{"location":"/*","tdm-reservation":1,"tdm-policy":"<terms URL>"}]`, and adds a `presentation.discovery` entry where `kind` is `tdm-policy` and `url` is the terms address, so a Theme can emit the matching `<meta>` elements. Omission emits none of these.

A publication's [Mintlify documentation](mintlify.md) applies the same reservation and crawler rules to its own host. A TDM reservation states the publisher's terms for text and data mining. Like crawler rules, it requests behaviour and does not restrict access to files.

## Browser manifest

```toml
[defaults.publication.machine]
web_manifest = { enabled = true, short_name = 'Example' }
```

`manifest.webmanifest` takes its name from the publication brand, its start address from the selected home or publication base, and its scope from that base. It uses browser display mode and includes the selected compiled site icon when present. `short_name` is optional. The output declares `application/manifest+json`; a Theme discovers its address through `presentation.discovery` where `kind` is `web-manifest`, and can emit a `<link rel="manifest">` element. This metadata does not add offline support or promise browser installation eligibility.

## Theme-owned missing page

A Theme can declare `render.not_found = 'not-found.html'`. This recipe inherits through explicit Theme dependencies and produces `404.html`. It receives the ordinary `presentation` context, including publication metadata, chrome, selected resources and discovery links, with `presentation.surface.kind = 'not-found'`. The top-level `site` field carries the publication site key. Its page-specific fields are `not_found.url`, `not_found.home_url` and nullable `not_found.search_url`. These are absolute addresses, so navigation still works when a host serves the page for a deeply nested unknown URL.

The missing page has no invented authored content identity. Its private render evidence records the template, model digest and exact output digest separately from authored pages. Native preview serves those same retained bytes with status 404 for an unknown page address. A Theme without this recipe keeps the preview's plain fallback and emits no `404.html`.

Related contracts: [workspace presentation](workspace-presentation.md), [Theme reference](theme-reference.md) and [preview](preview.md).
