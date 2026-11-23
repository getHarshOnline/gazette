# Feeds {#feeds-from-retained-publication-pages}

Generate Atom 1.0, RSS 2.0 or JSON Feed 1.1 from the same resolved pages that supply the Theme, search and JSON-LD. You configure which pages belong in the feed, and Gazette writes the feed files and gives the Theme their URLs.

Set a channel, format and article rules below, then expose the result through [Theme discovery](#theme-discovery). Add a lifecycle filter for public feeds so that drafts or archived pages stay out. For a publication walkthrough, see [Help readers find the work](https://rawthoughts.in/gazette/help-readers-find-the-work/).

## Explicit publication configuration

Configure feeds inside an existing brand publication's `machine` table:

```toml
[brands.publications.machine.feeds]
identity_base = "https://identity.example/publication/"

[[brands.publications.machine.feeds.channels]]
id = "feed:updates"
channel = "collection:home"
path = "/updates.atom"
format = "atom"
scope = { kind = "collection", recursive = true }
```

The referenced channel is an existing same-site content object. Its title, description, language, dates and attributions remain authored facts; the feed configuration does not declare them again. Names come from the shared principal catalog. `path` uses the existing file-route codec and the publication's serving base, including its mount prefix. Omission disables feeds; an explicitly empty channel list is invalid. Duplicate feed keys or output paths fail configuration, as do directory paths and conflicting identities across publications. All outputs share the site's collision and byte/count limits, including page, redirect, resource, graph and model files.

A publication can explicitly select dated published articles across all editorial residences. Channels may then omit `scope`, selecting that same-site candidate set:

```toml
[brands.publications.machine.feeds]
identity_base = "https://identity.example/publication/"
selection = { kind = "articles", residence = "all", lifecycle = ["published"], require_published_date = true }
omit_when_empty = true

[[brands.publications.machine.feeds.channels]]
id = "feed:updates"
channel = "collection:home"
path = "/updates.json"
format = "json"
```

The selector admits article-family pages, not collection or journey landings or documentation pages. It also excludes article-family pages inside a [documentation collection](content-model.md#product-documentation-collections). An explicit structural scope without this article selector retains its own selection rules. `lifecycle` must be nonempty and contain distinct native states. `require_published_date` tests the authored date's presence, retaining its calendar or timestamp precision. No date comes from file metadata. An explicit channel scope can narrow these candidates while retaining its traversal semantics; without a publication selector, every channel must declare its scope.

`omit_when_empty = true` omits the complete filtered empty feed in every format. No zero-byte placeholder appears in the output inventory or stage. The configured output path remains reserved against other feeds, files and directory prefixes. The default `false` emits an empty feed and requires that format's channel facts. Omission does not bypass channel or original candidate binding validation.

## Choose feed IDs and entries {#stable-identity-and-exact-selection}

Set `identity_base` to a stable HTTP(S) namespace you can keep if the publication moves. Gazette appends `#feed=` and the percent-encoded feed key for a feed ID, or `#content=` and the percent-encoded content key for an item ID. Changing a title, format, route, serving address or feed key leaves an item's ID intact. Changing its namespace or content key creates a new ID. An ID need not open in a browser. Declaring a namespace does not verify ownership. See [Atom's identity rules](https://www.rfc-editor.org/rfc/rfc4287#section-4.2.6).

| Scope | Complete entry selection |
| --- | --- |
| `{ kind = "site" }` | All other same-site content, in canonical URL order. |
| `{ kind = "collection", recursive = false }` | Direct collection members and structural children, in canonical URL order. |
| `{ kind = "collection", recursive = true }` | Those children plus descendant collections, journeys and their editorial members, deduplicated and in canonical URL order. |
| `{ kind = "journey" }` | The channel journey's exact authored member sequence. |

Gazette uses the channel for metadata and excludes it from entries. A missing, foreign or wrong-kind member fails the whole feed. An empty journey stays empty. Gazette resolves every candidate before applying publication filters. It never adds a date cutoff, limits the entry count, inherits the publisher as author or invents a missing date.

Site feeds also use the [discovery rules](render-inputs.md#discovery-without-weakening-materialization). A configured channel that cannot be discovered fails. A valid member that cannot be discovered stays out, without making the feed fail. Eligible entries keep their original order. Private compiler inspection applies structural and publication filters without hiding entries based on discovery.

## What each feed format emits {#wire-projections-and-lossless-metadata}

The private inspection result uses complete `PageData` for the channel and entries. Emitted feeds omit ineligible endpoints and raw container-member details. JSON uses `_gazette`; XML carries JSON text in `gazette:metadata` under `urn:gazette:syndication`. That technical namespace does not verify a subject or define a registered ontology. The serializer leaves out private source coordinates and keeps the existing lifecycle, relationship, attribution and source values. JSON exposes the feed ID in `_gazette_feed.id`, RSS in `gazette:id` and Atom in its standard `id`.

- Atom requires an explicit timestamp-valued `modified` on channel and entries. Each entry requires its own named authors; channel authors cannot fill a gap. An empty feed also requires named channel authors. Body HTML is escaped text with `type="html"`; explicit summary text remains separate.
- RSS requires the channel's explicit description. Entry descriptions contain full body HTML. Named authors use `dc:creator`, not a fabricated email in `author`. Standard dates are emitted only for whole-second timestamps; calendar dates and fractional timestamps remain exact in metadata rather than being promoted or rounded. [RSS field meanings](https://www.rssboard.org/rss-specification).
- JSON Feed emits full body HTML and text, optional explicit summary and exact timestamp fields. Standard root authors/language are omitted to prevent inheritance; item values are its own declarations. Unnamed author records remain in metadata, not invented names. No deprecated singular author alias is emitted. [JSON Feed fields](https://www.jsonfeed.org/version/1.1/).

Calendar-only dates never become midnight. No format consults a clock. XML character, namespace and syntax checks cover the complete output before success; carriage returns use character references to survive XML newline normalization. Byte limits bound wire output and temporary metadata encoding, not a process-wide allocation or CPU sandbox.

## Theme discovery

Site assembly exposes `presentation.feeds` only for successfully serialized outputs. Each entry carries `id`, `channel`, the authored channel `title`, resolved serving `url`, `format` (`atom`, `rss` or `json`) and `media_type`. The address comes from the same feed projection as its wire output, including a selected local preview base or publication mount. Themes need not reconstruct a feed route.

The list is shared by the site's page, alias, continuation, graph, taxonomy and mirror render contexts. Omitted empty feeds never appear; retained empty feeds appear only if their format validates and emits successfully. When no feed is emitted, or when rendering a standalone page without site assembly, the field is absent. Themes should guard it with `is defined`. This discovery contract does not alter selection, publication eligibility or the reserved output paths.
