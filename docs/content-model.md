# Shared content structure

Gazette gives each piece of writing an ID, a home site and a place in the structure of the thought it belongs to. A collection develops one question and can choose its pieces' display order, a journey sets the reading order an argument needs, and a related link or crossing lets a reader follow a connection. These relationships are declared separately so that renaming a page or moving its files does not change where it belongs.

| If you are… | Start here |
| --- | --- |
| Writing or organising a publication | [Your first publication](https://rawthoughts.in/gazette/first-publication/) |
| Resolving content in an integration | [Start with a collection and an article](#start-with-a-collection-and-an-article), then see [identity and relationships](#identity-and-relationships) |
| Building a product manual on a publication | [Product documentation collections](#product-documentation-collections) |
| Connecting layouts, indexes and page metadata | [Workspace and rendering consumers](#workspace-and-projection-consumers), [breadcrumbs and journeys](#structural-breadcrumbs-and-journey-neighbors), and [connected rendering](#connected-local-page-rendering) |

## Start with a collection and an article

Use the [workspace example](workspace.md#check-a-complete-small-workspace). It declares `site:notes` and `https://notes.example/`, the base for both routes below.

::::steps

:::step "Declare the collection"

Give the collection a stable ID, home site and route. Its body introduces the group.

```markdown
+++
[content]
id = 'content:notes'
site = 'site:notes'
title = 'Field notes'
lifecycle = 'draft'
route = { path = '/notes/' }
structure = { kind = 'collection' }
+++

# Field notes

Observations and the context needed to revisit them.
```

:::

:::step "Add the article by ID"

Give the article its own ID and route. Put `collections` before the content table, naming `content:notes`.

```markdown
+++
collections = ['content:notes']

[content]
id = 'content:observation'
site = 'site:notes'
title = 'Keep the context'
lifecycle = 'draft'
route = { path = '/notes/keep-the-context/' }
structure = { kind = 'page', family = { kind = 'article' } }
+++

# Keep the context

Write what you observed and the conditions that could change its interpretation.
```

:::

:::step "Check the relationship"

Pass both documents into the same corpus. Gazette resolves `content:observation` as a member of `content:notes`. Rename the article or change its route and it remains the same member. If you remove or misspell the collection, resolution fails.

:::

::::

For a reading sequence, declare an ordered journey inside the collection. A journey member cannot also declare direct collection membership. Use a related link to connect pages without changing where they belong or the order in which they appear.

## How the compiler resolves the graph {#native-graph-and-resolution}

The Rust SDK exposes `publication::content`, the core graph. The workspace compiler checks its site, project and principal references. Renderers, SQL storage and interfaces use the resolved relationships. Recalculating them in one consumer can make its result disagree with the others.

Gazette lowers the authored formats below into that graph. Rights, privacy rules and permission to publish come from the people responsible for the publication. The parser does not infer them.

## Identity and relationships

Each relationship carries part of the thought's structure. A collection develops one question from the whole, and a subcollection takes up one branch of it. A journey sets the order its argument needs. Editorial relations and crossings record the connections between pieces, so readers, search, feeds and machine-readable views can follow the thought as well as its containers. Project declarations say which product or inquiry a page is about or mentions. The table gives the rule each value enforces.

| Value | Structural meaning |
| --- | --- |
| Content node | Exact identity, resolved home site, title and independent draft/published/archived state |
| Page | Article, authored documentation, project, index, utility or data bound to an explicit extension schema |
| Collection | Optional parent collection; no folder-derived ownership |
| Journey | Ordered leaf members inside a required collection or subcollection |
| Collection membership | Sole direct collection residence, excluding journey containment |
| Primary journey | Sole containing journey; every member declares `primary = true` |
| Editorial relation | Exact content-to-content association; no containment, inheritance or ownership |
| Project declaration | Direct page/root-collection about/mentions or collection/journey context, with an exact project key |
| Resolved project link | One content-to-project about/mentions assertion, retaining all contributing declaration origins |
| Attribution | Author or rights-holder reference on one content object; eligible published objects may use a declared site author default |
| Language | Case-normalized BCP 47 prose-language tag, explicit on the content or inherited from its publication |
| Taxonomy membership | Direct exact `(taxonomy, term)` classification, separate from collections and project context |

Authored Markdown and structured landings may omit `content.site`. Gazette fills it from the exact configured source binding before resolving the graph. A source directory name, About relation, collection or journey never supplies the site. An unbound document must still name its site for local structural inspection; an omission without a source binding fails. An explicit site is preserved, and a conflict with the configured source still refuses source-bound publication. Derived values have no fabricated frontmatter location.

With publication `routing` enabled, an explicit `content.slug` and validated editorial placement supply a missing route. The configured Home derives `/`; other roots use the selected category prefix, and children append their own slug to their immediate collection or journey’s route. Existing explicit routes remain address exceptions. [Public routes](routing.md) describes the fields, cross-source placement and failure cases.

A leaf is standalone, directly contained by one collection, or contained by one journey. A journey member cannot also declare direct collection membership or join another journey. Every journey names its containing collection, including an empty journey. A journey's vector order is meaningful; unrelated declaration order is not. Editorial containment stays within the declared site. Related references may cross sites without moving either endpoint's home.

`Graph::resolve` rejects duplicate identities, edges or journey members; missing or wrong-kind endpoints; multiple residences; missing journey containers; non-primary journey memberships; cross-site containment; and collection cycles. It returns no partial graph. Ambiguous duplicate identities stop dependent relationship validation instead of letting declaration order select a body. Otherwise independent failures accumulate in stable order. Cycles are traversed iteratively, without a depth cutoff that silently drops ancestors.

Whole-set limits are checked before indexing: 16,384 nodes, 131,072 combined structural/editorial/project-declaration/attribution/classification edges and documentation navigation entries, and 16 MiB of identity, title, language, navigation labels and reference bytes by default. Independently, the same edge ceiling bounds total context traversal steps and unique derived project evidence. The text ceiling also independently bounds derived evidence's page/project/source key bytes, conservatively counting repeated map keys for every retained origin. Every examined structural edge consumes traversal work, even when its destination was already visited. Limits fail without partial context or output. Callers may supply other explicit limits; these are not renderer or whole-publication bounds.

## Choose the display order of a collection

A collection can put selected direct children first without making them a journey:

```toml
[content.structure]
kind = "collection"
member_order = ["content:observation"]
```

Each ID must already be a direct child through its declared collection membership or parent. The list can contain pages, subcollections and journeys. Listed children appear in the authored order, followed by unlisted children in identity order; omission or an empty list keeps identity order throughout. Unknown IDs, duplicate entries, the collection itself and descendants that are not direct children are rejected. Ordering references count against the graph's edge and text limits.

The resolved order reaches collection child cards, direct sibling navigation, Markdown collection contents and the default documentation tree. Public filtering retains the order of eligible children without exposing excluded IDs in shallow metadata. An explicit documentation outline or independently configured generated index keeps its own order. This field supplies no new membership, primary path, position or completion requirement. Journeys retain their separate sequence and completeness rules.

Selecting source declarations is distinct from filtering public output: an included collection retains its authored order references, so explicitly excluding a referenced child requires correcting that declaration too. Editor navigation and authoring slots preserve the exact original member-order scalar.

## Product documentation collections

A product manual can use collections and journeys without appearing among a publication's editorial essays. Declare its root collection's reader purpose:

```toml
[content.structure]
kind = "collection"
purpose = "documentation"
```

The root still has the ordinary required content identity and title, a resolved site and a route. Its subcollections, journeys and leaf pages inherit the documentation purpose through their declared containment. Their individual page families and content identities remain unchanged. An omitted root purpose means `editorial`; an omitted nested purpose inherits. A nested collection may repeat its inherited purpose but cannot contradict it (`collection_purpose_conflict`). A standalone documentation-family page is also treated as documentation.

`Graph::is_documentation` exposes that resolved classification. Article listings, project-article views and article-selected feeds exclude documentation, even when a contained page uses the article family. Root-collection listings likewise omit documentation roots. General search and all-page browsing can still expose eligible documentation pages. Purpose classifies the reading material; it does not hide it from every discovery surface or create access control.

Repository developer manuals assembled for Mintlify remain a separate input path and do not become these publication nodes. See [the manual target](mintlify.md) for that integration.

### Order and group a product manual

An optional collection `navigation` declares the manual's outline. It refers to existing content identities; groups arrange navigation without creating new pages or changing where those pages belong. For the collection and article above, the structure can be:

```toml
[content.structure]
kind = "collection"
purpose = "documentation"
navigation = [
  { group = "Start here", pages = [{ content = "content:observation" }] },
]
```

A content entry can name a page, journey or subcollection. Containers expand their own contents, preserving journey order and any more specific authored outline. Groups can contain further groups. Page titles and addresses come from the named content, rather than copies in the outline.

An authored outline must cover its collection's whole subtree exactly once. Missing, repeated, unknown or outside destinations, empty groups and invalid labels fail resolution. Outlines are available on documentation collections; they do not change editorial listings or turn group labels into parents. Without an outline, the resolved tree uses each collection’s member order and each journey’s authored sequence; unlisted collection children follow in identity order. Authored groups have a maximum nesting depth of 64. Resolved containment is traversed iteratively without a hierarchy cutoff; traversal and output limits fail instead of returning a partial outline.

The page projection exposes `documentation_navigation` on the documentation entrance and its descendants. It chooses the nearest ancestor with an authored outline, or the outermost documentation collection when none is declared. Its `root` carries the resolved entrance and `current` marks that entrance's own page. The ordered `events` form balanced `group-open`/`group-close` and `page-open`/`page-close` pairs. Group openings carry the label; page openings carry the resolved destination, `current`, `ancestor` and `has_children`. Page closings repeat `has_children` so a Theme can close a child list in one loop. The view contains no HTML and needs no recursive template or reconstructed tree. Discovery output filters destinations using the existing navigation policy, omits empty groups and recomputes child flags for that complete filtered view. Standalone documentation without a collection has no such outline.

## How project context reaches a page {#project-context-and-declaration-evidence}

Pages and root collections declare `about` or `mentions` for their own subjects; nested collections and journeys cannot. Collections and journeys may separately declare `context` for their descendants. Context flows down parent collections, containing collections of journeys, direct collection residence and the sole containing journey's ordered members. Editorial links, shared site, names, lifecycle and repository proximity contribute no inherited project meaning. `Graph::editorial_parent` exposes the immediate declared parent; `editorial_ancestry` borrows the object followed by its complete ancestry, nearest first. Both use one validated parent index, not source paths or alternative reading links.

`Graph::project_relations` supplies the resolved content-to-project map. Direct about wins over mentions of that same project, but direct mentions and inherited context remain in the link's evidence set. About and mentions never propagate; both remain local when declared on a root collection. Use a separate Context declaration when the intended relationship should reach descendant pages. Equivalent traversal paths retain one declaration origin, not an exponentially growing list of paths. Each origin names its source content key and original declaration kind; source byte coordinates remain in the retained authored document, not reconstructed by the renderer.

Known pages with no project links return an empty map. A root collection with a direct About or Mentions declaration also has its own resolved entry. Unknown keys and containers without such a direct entry return none. Original declarations remain available via `declared_projects`, including context that currently reaches no page. This is structural meaning, not proof that a private project or draft page can be disclosed publicly.

### Keep project facts on their landing

A workspace may select one authored landing as the owner of a project's facts:

```toml
[[projects]]
id = "project:example"
landing = "content:example-home"
```

That landing supplies a singular `project` table alongside its content metadata. The plural `projects` array still declares About, Mentions and Context relationships; it does not supply the catalog record. In a Markdown landing:

```toml
+++
projects = [{ project = "project:example", kind = "about" }]

[project]
id = "project:example"
name = "Example"
lifecycle = "paused"
visibility = "public"
availability = "gated"
repository = "https://github.com/example/example"
license = "MIT"
available_through = { site = "site:services", content = "content:enquiry" }
resources = { learn = "content:example-help" }

[content]
id = "content:example-home"
site = "site:example"
title = "Example"
lifecycle = "draft"
route = { path = "/example/" }
structure = { kind = "page", family = { kind = "project" } }
+++
# Example

Explain the project and what a reader can do with it.
```

The example requires the named sites, enquiry page and help content in the same resolved publication set. In a structured landing, the identical facts use a root YAML `project:` mapping beside `content:` and `projects:`. Both formats share the native `Project` schema, including optional kind, public entity identity, name, repository, licence, origin, discussion and resources.

Project lifecycle is `planned`, `active`, `paused`, `done` or `closed`; visibility is `public`, `unlisted` or `private`; availability is `not-applicable`, `unavailable`, `gated` or `available`. These are independent facts. A paused project can remain publicly described and obtainable through an engagement. The landing's own `draft`/`published`/`archived` lifecycle governs that content object separately.

`available_through` names an exact site and routed content identity where a reader can enquire about gated access. The target must belong to that site and be present in the captured graph. It produces an `available-through` resource action with the target's native serving address and link facts, including preview addresses. It is valid only with `availability = "gated"`; it neither promises service nor changes either endpoint's lifecycle, ownership or publication approval.

A selected landing must be a project-family page, root collection or the exact index-family Home selected by its publication, directly About the same project, with its own canonical route. A programme can therefore keep its publication Home and presentation; an ordinary index or a root address alone does not qualify. Its captured source must belong to its declared site, and any primary project placement must agree with that home. Missing landings or facts, duplicate identities, conflicting landing selections, unselected fact records and mixed workspace/landing facts fail the whole resolution. Gazette reads the already retained metadata once; unsaved editor replacements use that same join without reopening files. A workspace can alternatively retain the complete legacy `[[projects]]` record, but cannot combine it with `landing` or another authored record for that project.

`project.kind` declares what the catalog subject is, independently from lifecycle, visibility and availability. Supported values are `software-application`, `software-source-code`, `creative-work`, `collection`, `project` and `research-project`; omission leaves it unknown. Project and research-project mean organizational undertakings, not generic labels for anything in a repository. `project.identity` optionally supplies a public HTTP(S) entity IRI distinct from the presenting page. Otherwise the resolved canonical landing supplies its address plus `#entity`; an unanchored subject retains a document-local identity. See the [public mapping and linked-work rules](json-ld.md#explicit-project-subjects).

`ResolvedContent::project` and `projects` expose the complete joined catalog shared by pages, search, graph, listings, JSON-LD and the private registry. Configuration-only callers use `Workspace::project_declarations` for every declared identity; `Workspace::project` and `projects` expose only complete inline facts. A landing selection never manufactures an incomplete project record. Documentation-only planning can use the configured identity without acquiring publication content. Joined catalog count and serialized bytes have independent content node/text limits.

### Choose Learn and derive Docs

`project.resources.learn` selects a documentation-family page or a page/collection within a documentation collection, including article-family guides. A link to the current page or the project's own landing is omitted. Leave Learn absent when there is no useful distinct help entrance.

For Docs, configure `documentation_repository.public_base_url` beside the existing documentation repository's `name`, `path` and `build`. Gazette selects the build module with the exact project identity and joins that base to the first page of its first manual navigation section. The target's own route normalization applies, including Mintlify's reserved `index` spelling. No host name, repository name, module slug or source filename supplies a guessed address. No public base or matching module means no derived Docs action; multiple matching public modules or an entrance that starts with generated reference fail resolution. This selection establishes a configured destination, not proof that it has been published.

Landing-owned facts cannot repeat `resources.code_documentation`; the configured manual owns that address. A full inline workspace project may retain an explicit external Docs URL for compatibility, but it must agree when a configured module also supplies one. See [the manual target](mintlify.md) for manual selection and assembly.

## Resolve pages against the workspace {#workspace-and-projection-consumers}

Call `Compilation::resolve_content` after supplying the authored graph. It checks the home site for each node, every declared project and each direct attribution. A principal reference must name a principal, even when a site, project or brand uses a similar key.

| Diagnostic                    | What to fix                    |
| ----------------------------- | ------------------------------ |
| `content_site_reference`      | The content node's home site   |
| `content_project_reference`   | Its declared project reference |
| `content_principal_reference` | Its attribution reference      |

Each diagnostic names the declaring and target keys. These checks apply to drafts, archived content and context on an unused container. A private, historical or unavailable project can be a valid internal reference. Check its disclosure separately.

`ResolvedContent` owns the graph and borrows the workspace compilation. Use `compilation`, `graph` and `site_for` to read it. `Graph::journey_view` returns the ordered sequence as borrowed nodes, including any draft or archived members. Consumers share that order and the supplied titles. Filter the view before exposing it publicly. Tera receives presentation data and does not query the graph or database.

When changing structural resolution, run:

```sh
cargo test --locked -p gazette-core --test content_graph --test project_relations
cargo test --locked -p gazette-compiler --test workspace_content
cargo test --locked -p gazette-sdk --features native-local --test content_projection --test project_projection
```

These catch multiple residences, missing or cyclic relationships, changed About/Context precedence, lost declaration origins and disagreement between template and JSON projections. The [JSON-LD reference](json-ld.md) describes their structured-data serialization.

## Structured landing sources

A page's optional `template` selects its presentation first. A composing site's `composition.page_templates` binding applies when that source override is absent; omission of both uses the selected Theme's declared presentation-mode or content-kind entry, then its ordinary page entry. The choice preserves content identity, body and source provenance. See [workspace selection](workspace-presentation.md#per-content-presentation-selection).

A file named `*.landing.yaml` or `*.landing.yml` selects the native structured landing parser. Other YAML files remain opaque assets. The root declares `format: gazette-landing/v1`, the same `content` metadata and optional `template`, `collections`, `related`, `project`, `projects`, `resources` and `notebooks` as Markdown, and a nonempty ordered `sections` list. The workspace’s selected Markdown profile (including `publication-math`) applies to every embedded body.

```yaml
format: gazette-landing/v1
content:
  id: content:introduction
  site: site:example
  title: An introduction
  lifecycle: draft
  route: { path: /about/ }
  structure: { kind: page, family: { kind: utility } }
sections:
  - component: introduction
    heading: { level: 1, text: An introduction }
    body: A premise in ordinary Markdown.
  - component: comparison
    arguments: { mode: paired }
    sections:
      - heading: { level: 2, text: First view }
        body: The first account.
      - heading: { level: 2, text: Second view }
        body: The second account.
      - body: The conclusion belongs to this comparison.
```

Components and their argument schemas must exist in the selected Theme. A section may have `component`, `arguments`, `heading`, `body` and nested `sections` only. A component may obtain its entire presentation from arguments; it does not require filler prose or child sections. An unnamed section must contain a heading, body or children. Nested sections use the same rules. A named child selects a component from the effective Theme and uses its own argument schema. Arguments require a component and must be representable as native TOML component data; null and out-of-range integers are refused. Headings are plain single-line text with level 1–6. Markdown bodies share one page-wide heading, reference and footnote scope. An unclosed fenced block cannot consume the next declared section.

The original YAML bytes retain source identity. One compiler-owned normalized body feeds the existing component, reference, asset, search and machine-output paths; there is no generated Markdown companion to maintain. Source diagnostics map back to containing YAML constructs, without claiming exact editable string ranges. Duplicate keys, aliases, anchors, tags and multiple documents are refused. Existing corpus/document limits bound source bytes, values, nesting and normalized output. `index.landing.yaml` can own a leaf page package under the same boundaries as `index.md`; a root Home does not absorb the whole content tree.

Editor assistance uses original YAML scalar locations for component names and schema-defined argument keys or closed values. Hover can show the selected component's template, argument schema and available child-section choices. Normalized Markdown bodies have a different coordinate space and do not receive inferred inline edits. Selected-set parsing and cross-document references retain the original YAML source coordinates.

## Owner-bound discussion

An optional `discussion` atom belongs directly to a site or project. It names an existing composed `capability`, opaque `repository_id` and `category_id`, and bounded public `options`. It is not inherited or merged across placements. The public repository address comes from the site's brand publication `repository`, or the project's existing `repository`; it never comes from a private Git destination remote.

Workspace validation rejects incomplete atoms, missing public repositories and unknown capability references. A project's capability must resolve in each rendered placement. Explicitly disabled capability selections suppress the page binding; enabled selection is not proof of provider activation or consent.

Article and documentation pages use their already resolved native source owner. Project-owned content requires that project's atom and never falls back to the site. An ordinary site article uses its site's atom. Other page families, collections and journeys have no resolved discussion binding. About, mentions and editorial relationships do not choose a discussion repository.

The borrowed page `discussion` view supplies `owner`, `repository` and `binding` to Theme and page-data consumers. The private registry preserves the configured site and project atoms independently of rendered selection. Theme implements presentation; provider installation, public visibility, category qualification, pathname uniqueness, consent and publication approval remain separate evidence.

## Structural breadcrumbs and journey neighbors

Pages and root collections may declare root metadata `browsing = ["content:directory"]` (the same `browsing` sequence in a structured landing). Each exact target is an existing, same-site, routed index page. Duplicate, self, foreign-site, wrong-kind, missing, unrouted or republished-index targets refuse the complete graph. Multiple distinct targets are valid; their identity order is deterministic, not a primary path. These source-owned associations have their own native graph edges. They establish offered browsing paths, never arrival history, editorial residence, project context or inherited ancestry. Omission preserves absence. Source-owned selection retains or removes the declarations with their declaring object; discovery suppresses each public edge unless both endpoints are eligible. Shallow content nodes carry no directory ID list. The existing graph edge/text and complete projection bounds apply independently before retaining results.

`PageData.breadcrumbs` carries the sole editorial path as `{ via, items }`. `via` is the exact direct container key; `items` is root-first and ends at the selected node. Each item borrows the node and its optional resolved route. Nested containers use their declared ancestry; uncontained content has a self-only path. There is no invented Home, primary collection, folder hierarchy or route fallback.

The existing `journeys` records retain the declared primary flag and include the exact journey route, zero-based `position`, full `total`, ordered `members`, and immediate `previous`/`next` endpoints. First and last boundaries are null. An unrouted neighbor remains a node with a null route; it is not skipped to reach a clickable record. Draft and archived members retain their positions. Foreign-site references are not journey members. A journey's containing collection contributes a path, not a new direct membership. Each `members` entry has the same `{ node, route, presentation }` contract as a neighbor, including the current article. The whole sequence is resource-bounded; discovery omits the membership if any member is ineligible rather than leaking its title or renumbering a partial list. Discovery nodes remain shallow.

A page directly residing in a collection receives `collection_navigation`: the exact `collection` endpoint and its direct `members`, each using the same navigation link contract. Members include the selected page and follow the collection’s resolved member order; these remain reading choices with no sequence or progress measure. Nested collections and journeys remain destinations rather than expanding their descendants. Discovery filters ineligible siblings and emits shallow nodes. A journey resident receives its sequence instead, and a standalone page has no collection navigation.

Content records in `collections`, `children`, `steps` and `related` additionally carry optional `resolved_route`, borrowed from the same compiler result. Their existing `route` remains the authored declaration. Use `resolved_route.url` for the serving destination; `canonical_url` identifies its origin when republished. An unrouted record retains its place without a `resolved_route`; themes must not construct a URL from its identity or authored path. Collection order and declared journey order are unchanged. Discovery-safe records retain these addresses only after normal eligibility filtering, without exposing container member declarations.

The compiler computes these views once from the resolved graph. Theme, page JSON and search borrow the same data; [JSON-LD](json-ld.md) translates the same ordered paths. Workspace menu selection, Theme presentation and public admission remain separate. None of this local draft-inclusive evidence approves disclosure.

Before retaining each navigation endpoint or related content view, the compiler charges an occurrence and its serialized node/route bytes to independent per-page allowances using the content edge and text limits. This bounds ancestor-depth expansion, not just the input graph. Exceeding either returns `authored_projection_limit` without a partial page. These allowances do not replace the later complete page-context and site-output limits.

### Decorative artwork for generated listing items

An owning authored document may declare root `listing_artwork` beside `[content]`. It has one required `default` and optional `light`/`dark` references. Each is a strict `{source, entry}` object using an existing source key and literal capture-relative EntryPath. This is not a collection membership, inherited cover, Theme selector or alternative text. Both framed TOML and landing YAML use the same native Metadata schema.

```toml
[listing_artwork]
default = {source="source:articles", entry="media/default.webp"}
light = {source="source:articles", entry="media/light.png"}
```

The exact source must already be selected as an unambiguous same-site content binding. References cannot select an otherwise absent source or trigger acquisition. The entry must be a retained regular opaque static image, not retained-only, and have an existing asset-plan output. Supported suffixes are png/jpg/jpeg/gif/webp/avif, case-insensitively. This reuses byte-preserving static transport: it does not add decoded image/MIME safety validation, transformation, copying, remote URLs or SVG capability admission.

Candidate selection includes declared images only as dependencies of included owners inside already retained sources; explicit exclusion of a required image refuses. The optional view reaches only eligible ordered listing items, using native public/preview asset URLs and no private source coordinates. Missing mode variants remain absent and use the default at presentation time. The field does not expand shallow ContentNode, graph, search or JSON-LD meaning. See [Theme reference](theme-reference.md#generated-reading-context) and [static transport](render-inputs.md#static-assets-from-configured-sources).

Structural card views (`children` and ordered `steps`) also carry optional resolved artwork. For these views alone, an absent selection uses the nearest explicit editorial ancestor's selection, through the validated single-residence chain. A child declaration overrides it. Public projection stops at an undiscoverable ancestor; it never searches through that boundary. Related links, project relationships and filesystem locations do not supply artwork. Traversal, rendition occurrences and retained URL bytes have shared page limits. This is a decorative presentation default: the child's identity, route, ownership and JSON-LD meaning remain unchanged.

## Direct attribution, not identity invention

Each content object can declare ordered `attributions` using exact principal keys:

```toml
# Inside [content]; each principal must also be declared in the workspace.
attributions = [
  { principal = "person:writer", role = "author" },
  { principal = "organization:holder", role = "rights-holder" },
]
```

The core `Attribution` type is used directly, not redefined in frontmatter, templates or interfaces. Declaration order is preserved, including byline order. A repeated identical principal/role pair fails with `content_duplicate_attribution`; the same principal may explicitly hold both roles. Every role reference consumes the graph's shared edge and key-byte budgets before validation/indexing.

These are direct claims about the declaring object. Collections, journeys, editorial links, project placement and the site's publisher do not supply missing roles. A person or organization can be explicitly referenced; neither form itself proves authorship or rights. A published article with omitted or empty attributions can use its site's explicitly configured `default_author`. Any existing attribution prevents that fallback, and draft or archived objects receive no default credit. The [site principal rules](workspace-publications.md#independent-site-responsibilities) define this scoped policy. No contact lookup, guest creation, publisher-author fallback or rights inference runs.

`PageProjection.data.attributions` borrows only those principals referenced by the selected node, with their exact roles in authored order. An explicitly selected principal website accompanies each role as `website.url` for navigation and `website.canonical_url` for public identity metadata. Its native `destination` contains the exact link kind, optional target site and selected target presentation relative to the rendered page or listing. Generated listing items carry the same resolved roles and website addresses rather than rejoining names. The graph retains the same declarations under `content.attributions`; no global principal inventory is copied into the page. `site.publisher` remains an independent configured reference. Original role/principal fields remain addressable through `ResolvedAuthored::origin`, including array indices, Unicode and CRLF byte coordinates. An inherited credit has no invented span in the authored document; its selection belongs to the site's `default_author` field.

The optional [principal display name](workspace-publications.md#principal-display-names) travels with the same borrowed record into page, search and graph projections. JSON-LD emits it for the resolved role holder; a missing name remains absent, and a publisher reference does not become a byline.

This is structural binding for local inspection, not approved public attribution. Profile fields, consent, rights scope, licence, validity/revocation and admission evidence remain separate work. Neither an `author` nor a `rights-holder` declaration grants permission to publish. Controller, operator, processor and canon-authority roles are not aliases for these content roles. Explicit [site responsibilities](workspace-publications.md#independent-site-responsibilities) travel separately under `site.roles` and `gazette:siteRoles`; they never populate content attribution or feed authors. When changing attribution, run `cargo test --locked -p gazette-core -p gazette-compiler --test attribution` and `cargo test --locked -p gazette-launcher --test publication_render attribution`. Check duplicate roles, exact principal binding, published-only site defaults, explicit overrides and agreement between the byline and JSON-LD.

## Language and explicit classifications

The [workspace catalog](workspace-discovery.md#taxonomy-catalog-and-language-scope) owns taxonomy identities, labels, optional language constraints and scoped terms. Content records only their exact membership pairs, using the core `TaxonomyMembership` type. Given that catalog and a declared `example:site`, this is a complete framed article input:

```toml
+++
[content]
id = "content:classified"
site = "example:site"
title = "Publication systems"
lifecycle = "draft"
language = "EN-us"
taxonomies = [
  { taxonomy = "taxonomy:subjects", term = "term:systems" },
]

[content.structure]
kind = "page"
family = { kind = "article" }
+++
# Publication systems

A shared publication model keeps each article's identity and relationships consistent across its page, search entry and feed.
```

`language` is optional and uses the shared `LanguageTag`: BCP 47 syntax with normalized subtag casing, so this value becomes `en-US`. Invalid or over-limit tags fail; there is no body detection, filename inference, programming-language mapping, translation identity or implicit English default. Omitted language uses the publication's configured language when present; otherwise it remains absent. An omitted or empty `taxonomies` array remains empty; both empty classifications and absent language are omitted from serialized content nodes.

Memberships form a sorted set of exact pairs. Duplicate pairs fail with `content_duplicate_taxonomy`; several distinct terms in one taxonomy and the same term key in different taxonomies remain distinct. Workspace binding rejects unknown taxonomies (`content_taxonomy_reference`), unknown terms within the named taxonomy (`content_taxonomy_term_reference`) and violations of its language scope (`content_taxonomy_language`). A language-scoped taxonomy requires the content's exact normalized tag; missing language or a broader/narrower tag is not a match. A language-neutral taxonomy supplies no default or restriction. These checks apply to draft, published and archived nodes alike and return no partial graph.

Collections, journeys, editorial links and project context do not propagate language or classifications. A placement's `native_markdown.language` applies when lowering unframed project Markdown. The publication's language then supplies any still-missing content language, including in framed content. Native lowering supplies no taxonomy memberships. Its separate derivation evidence records `language_from_profile` only when that profile supplied the value; inherited publication language has no fabricated authored span.

`PageProjection.data.classifications` and `SiteGraph.classifications[content_id]` use one scoped classification join. Each item has `taxonomy = { id, label, language? }` and `term = { id, label }`, borrowing the configured labels for only the declared pairs. Each view may additionally carry `destination = {url, canonical_url, destination}` for its exact configured term listing; the taxonomy view may carry the equivalent destination for its explicitly configured used-term index. Both reuse `publications[].taxonomy_pages`; no new route is derived from a key or label, and a taxonomy index is not a missing term's fallback. `url` follows selected serving/preview addresses while `canonical_url` retains the public base. Native link facts identify the actual target site/world, relative to the rendering source. No destination creates a Content node, changes a term identity, or asserts subject membership. No unrelated terms or whole catalog are copied. Pages receive their own classifications; site graphs include classifications only for local nodes, never foreign endpoint records. Empty views remain empty arrays. Tera and machine JSON consume the same projection rather than looking up or reconstructing labels. The graph charges each emitted classification edge and its taxonomy/term records, including repeated occurrences, plus the complete encoded model byte budget. Configured classification destinations also precharge their repeated serving/public URI bytes before allocating the view, sharing the page, recommendation or graph text ceiling; excess fails instead of truncating the catalog view.

Original authored language/membership fields remain addressable through the retained document origin API, including original array order before sorting. Workspace label coordinates use the same retained workspace AST. Neither normalized values nor defaults invent source spans. Explicitly configured taxonomy and term landing pages reuse that scoped join for Theme, navigation, JSON-LD and `llms.txt`; absent routes are never inferred and no synthetic content node is created. This is still local structural and presentation behavior, not public admission or a general faceted-search contract.

After changing classifications, run `cargo test --locked -p gazette-compiler --test workspace_taxonomies --test authored_projection --test discovery` and `cargo test --locked -p gazette-launcher --test publication_render taxonomies`. Check language-scoped membership, exact labels and configured term destinations in the emitted pages.

## Read declared structure from source files {#retained-document-lowering}

The SDK's `authored::resolve` consumes a compiled workspace and a borrowed `ParsedCorpus`. It deserializes retained TOML syntax without reparsing source, produces the existing graph declaration and calls `Compilation::resolve_content` once for the complete set. Aggregate documents before this call when references span sites; independently resolving partial site graphs cannot prove closure.

The structural profile uses the core node schema directly under `[content]`, with root-level `collections`, `related` and `projects` arrays for outgoing membership, editorial and typed project declarations. A project entry is `{ project = "project:example", kind = "about" }` on a page or root collection, `kind = "mentions"` on a page or root collection, or `kind = "context"` on a collection/journey. Root arrays must precede TOML tables; they are lowered once into the core declarations, using the enclosing content key as source. Container parents and ordered journey members remain in the node's typed `structure`, not a second relationship model. For example, this is one leaf input when its referenced collection is also supplied:

```toml
+++
collections = ["content:notes"]

[content]
id = "content:example"
site = "site:example"
title = "Keep the context"
lifecycle = "published"

[content.structure]
kind = "page"
family = { kind = "article" }
+++
# Keep the context

Record the instrument, conditions and original reading before interpreting the result.
```

Unknown fields fail explicitly; owner evidence and public-admission policy remain separate inputs. No permissive extra map quietly accepts unsupported fields as implemented. Missing relationship arrays mean no direct declarations; absence does not suppress inherited container context. Exact absence remains visible in retained syntax. This project-specific union rule does not establish a generic policy for replacing other inherited arrays.

### Controlled domain data

Workspace `[[extensions]]` declarations select an exact `id` and local `schema`. The schema uses JSON Schema Draft 2020-12, expressed as TOML data. External schema retrieval is refused. An extension page declares `family = { kind = "extension", schema = "schema:example", data = { count = 3 } }`. The compiler binds and validates that data against the selected schema before returning any resolved content. Unknown schemas and invalid values refuse the complete join; validity does not establish truth, rights or publication approval.

An extension may carry generic admitted presentation intent alongside its data:

```toml
family = { kind = "extension", schema = "schema:example", data = { count = 3 }, presentation = { mode = "mode:immersive", amplitude = 3, variant = "variant:night" } }
```

`mode` and optional `variant` are owner-namespaced keys. `amplitude` is an integer from zero through four. These coordinates travel with the admitted content record; Gazette passes these keys through without interpreting them as a world or selecting a Theme package. The author declares them explicitly for the content.

The original values stay in the shared content structure consumed by Theme, graph and JSON-LD projections. JSON-LD retains them as a technical JSON literal, not invented Schema.org assertions. Serialized data counts against the existing content-byte allowance. Workspace navigation retains the original schema-field coordinates, and the private registry retains one schema declaration plus the page's exact data.

When changing extension data, run `cargo test --locked -p gazette-compiler --test extensions` and `cargo test --locked -p gazette-adapter-sqlite registry::tests::extensions`. Check schema binding, exact persistence, replacement, removal and invalid-data refusal.

`ResolvedAuthored` borrows every original body and asset and indexes content keys to their source paths. `document` returns that original parsed document; `origin` resolves a typed key/index path to full-document UTF-8 coordinates and the exact source digest. Missing/defaulted fields get no invented location. Metadata coordinates account for LF/CRLF delimiters and Unicode without invoking Markdown output inspection. Structural errors retain all declaration locations, including both sides of duplicate identities, rather than keeping the last file.

Unframed Markdown uses an explicit project placement's [native profile](render-inputs.md#plain-project-native-markdown), preserving its source and deriving structural metadata without inventing attribution. Without that binding it returns `NativeBindingRequired`, except for an exactly owned article `notes.md` companion described below. Project READMEs do not need fabricated frontmatter. Assets remain accounted for as exact bytes; JS, TS and notebook presence does not imply compilation or admitted execution. File/schema failures return no successful framed-only subset.

## Article notes stay with their article

A directory-form article uses `index.md` and may own an unframed `notes.md` beside it. The same rule applies when the article bundle is the explicitly selected source root. Notes are commentary, not another content node, canonical route or search result. A framed companion or another unowned Markdown document does not acquire this exception. Container child pages remain independent.

The page and search models expose `notes` separately from the main body, with `body_html`, `body_text`, headings, components, links and the retained byte digest. The owning article remains the only content identity. Private source paths stay in operator evidence. Theme may put the notes in a sidebar or panel; Gazette does not append them to the continuous article or primary search body.

A page's declared [notebooks](notebooks.md#notebooks-in-notes) join its Notes after any `notes.md`, as structured cells in `notes.notebooks`; their text joins `notes.body_text`. A page with notebooks and no `notes.md` still has Notes, with empty `body_html` and no retained byte digest.

Notes reuse retained Markdown rendering, selected components and the article's source-relative resource binding. Their anchors use `gazette-notes-`, including local footnote anchors. A notes H1 is lowered one level and the emitted heading hierarchy is recomputed, keeping commentary subordinate without changing source bytes. Primary headings cannot occupy the reserved notes namespace. Ordinary fragment links inside notes address that companion; symbolic references can address either the article's main anchors or its namespaced notes anchors.

When changing article Notes, run `cargo test --locked -p gazette-compiler --test article_notes` and `cargo test --locked -p gazette-launcher --test publication_render article_notes`. Check companion ownership, resources and namespaced headings, including a missing anchor followed by a corrected request.

## Resolve ecosystem links {#ecosystem-links-resolve-retained-targets}

Publication Markdown links to stable content identities with native wikilinks. Keep the complete existing key; the label, title, source location and publication address can change independently:

```text
[[id:content:guide#detail|Readable label]]
[[id:collection:systems|Systems]]
```

The `id:` selector looks up that exact key in the retained graph, then uses its current serving route. It never guesses a namespace, creates an identity alias or acquires a missing target. A missing or unrouted target fails. Escape a literal `%` or `#` within a key as `%25` or `%23`; the identity is decoded exactly once before lookup. The optional `#fragment` still uses the target’s native main-body or notes anchor index.

Existing domain-and-route selectors remain supported:

```text
[[site:example.test/guide/#detail|Readable label]]
```

The domain selects an explicitly configured publication, and the path selects an actual declared route or alias. Aliases resolve to that target's primary serving route. The fragment must exist in the retained main body or namespaced article notes. Unknown or ambiguous domains, missing routes, unsafe selectors and missing anchors fail with the original source construct. Code literals are not links; references in omitted footnotes still undergo validation. No target is fetched or added to the selected build merely because it is linked.

Public and preview addresses use the same proved target identity and the selected serving base. The resulting anchors and page metadata share the ordinary link annotations. The `id:` and `site:` spellings are admitted only through native wikilink syntax, not as an arbitrary URL in an ordinary Markdown link. Existing source-relative documentation and resource links keep their separate contract.

Authored publication bodies and article notes reject relative `.md` and `.markdown` file links, including encoded and differently cased suffixes. Use a public address or a checked publication reference. Repository manuals and explicitly mounted native documentation retain their source-relative Markdown links and target checks.

Shared authoring analysis retains the exact selected source revisions and full-file construct locations for navigation and backlinks. A missing target returns the compiler's located failure, not a guessed URL or a successful partial reference set. An unresolved link nested in a footnote cannot navigate to the broader footnote instead; its independently valid label remains usable. Source-bound symbolic destination completion is available through the shared authoring analysis, MCP `authoring_complete_reference` and LSP completion. It proposes selected page, heading and article-owned notes targets without changing the visible label. Failed current target evidence never borrows an earlier graph; accepted edits require reanalysis.

When changing these links, run `cargo test --locked -p gazette-compiler --test symbolic_references` and `cargo test --locked -p gazette-application --test authoring_symbolic`. Check exact target/anchor resolution and current-source navigation after edits.

## Authored description and chronology

Article pages may declare `keywords = ["retained inputs", "publication graph"]` and `schema_type = "TechArticle"` inside `[content]`. The other article subtype values are `Article` and `BlogPosting`. These describe the contribution, not its residence, taxonomy, lifecycle or approval. Keywords retain their authored order and must be distinct, nonempty plain-text phrases with no surrounding whitespace or control characters. Non-article families cannot declare these article fields. `SchemaType` also admits `AboutPage` under the utility-page contract below; `ArticleType` remains a Rust compatibility alias.

### Declare an About page or a service subject

An About page stays in the `utility` family. It may name an existing principal explicitly:

```toml
[content]
schema_type = "AboutPage"
subject = { kind = "principal", principal = "person:example" }
structure = { kind = "page", family = { kind = "utility" } }
```

When supplied, the principal's catalog record supplies its person or organisation kind, name and declared website. This does not make it the page's author, publisher or project subject. An `AboutPage` may omit a subject, including for a brand or programme described through existing project relationships. Omission invents no principal or `mainEntity`. Neither subtype nor subject is inferred from a title, route, Theme or visual page mode.

Use `ProfilePage` for a person's identifying About page on their declared website. It requires a person as its principal subject. One published profile per person supplies their public profile URL and byline link. Their graph ID still uses the canonical website root and its `#person` suffix.

A service page also stays in the `utility` family, with `schema_type` omitted so the page remains a `WebPage`. Its service has a separate explicit identity and facts:

```toml
[content]
subject = { kind = "service", id = "service:example", name = "Workflow review", provider = "organization:example", description = "Review one agreed workflow and its handoffs." }
structure = { kind = "page", family = { kind = "utility" } }
```

Service names and descriptions come only from that declaration; description is optional. The provider must be an existing principal and never falls back to the publisher. A repeated service ID is allowed when its name, provider and optional description agree exactly; conflicting declarations fail with their owning pages identified. Page chronology and lifecycle remain page facts. Gazette does not infer service offers, prices, availability or dates.

These declarations work in TOML frontmatter and YAML landings through the same content model. They do not change project `about` and `mentions`, listing or journey ordering, or visual page modes. The complete subject participates in graph and registry semantic identity. These page-subject values remain in the retained workspace inputs and semantic digest without changing the normalized registry rows or release history. They do not gain independently queryable SQL columns or foreign-key checks. The compiler checks principal references before projecting them. Omitted subjects retain the previous serialized model and hash.

Omitted keywords and subtype remain undeclared; containers supply neither. The native node carries them into page, search and graph metadata. JSON-LD uses the explicit subtype, or its ordinary `Article` family mapping when absent, and emits declared keywords. Browser search indexes those phrases separately from taxonomy membership. `gazette schema authored-content` and MCP `contract_schema` with `contract = "authored-content"` expose the same native declaration schema used by frontmatter assistance.

Optional `content.description` is authored plain text, not Markdown, a body summary or the site description. Nonempty multiline text is preserved exactly; blank values and control characters other than tab, CR and LF fail graph validation. Templates remain responsible for context-appropriate escaping.

Optional `content.published` and `content.modified` use one `EditorialDate` value. Both JSON and TOML use strings; quote TOML dates rather than relying on TOML-specific datetime coercion:

```toml
[content]
# Other required content fields are omitted from this fragment.
description = "An explicit description, separate from the body."
published = "2026-09-10"
modified = "2026-09-10T18:30:00.125+05:30"
```

The calendar-date form is exactly `YYYY-MM-DD`. The timestamp form requires uppercase `T`, seconds, and either uppercase `Z` or an explicit `±HH:MM` offset. Years are 0001–9999 and offsets must be within ±14:00, inclusive, so every accepted value also fits XML Schema date/dateTime publication outputs. An optional fractional second has one to nine digits. Calendar/time validity is checked before the date enters the shared model. Timezone-less timestamps, unknown `-00:00` offsets, leap seconds, extended years and excess fractional precision fail instead of being guessed or truncated.

Serialization preserves date-only precision and nonzero offsets, normalizes zero offsets to `Z`, and renders fractions with zero, three, six or nine digits without losing nanoseconds. Original spelling and coordinates remain in the retained TOML AST. `chronological_cmp` compares dates by calendar day and timestamps by instant; mixed precision is incomparable, not midnight in an invented timezone. Equal instants with different explicit offsets retain their distinct representations. There is no imposed cross-field ordering: the two fields are independent authored facts, not a release-event sequence. Reading indexes use their documented [display order](workspace-discovery.md#generated-reading-indexes) for mixed precision.

Absence stays absent. Native unframed documents do not acquire dates or a description from filenames, filesystem/Git observations, profiles or the clock. No value changes content lifecycle, schedules publication, grants release authority, or falls back from `modified` to `published`.

The existing `ContentNode` is the single owner. Graph limits charge description and canonical date bytes; retained-source limits separately bound the input. Tera's `content`, page JSON, search and local graph reuse those same fields. The [JSON-LD projection](json-ld.md) maps present root facts to `description`, `datePublished` and `dateModified`, using explicit date/dateTime value types. Shallow linked JSON-LD records do not acquire another page's editorial metadata. The [body summary](#explicit-body-summaries) is a separate retained-syntax projection. Feed timestamp policy is implemented by the shared feed projection: timestamps retain their instant, while date-only values are not invented as midnight. Public admission remains separate; editorial metadata does not claim to authorize it.

## Explicit content licence

For the authoring task, see [Record the content licence](https://rawthoughts.in/gazette/credit-and-reuse/#record-the-content-licence).

Optional `content.license` is an exact HTTP(S) URI for the declared content licence. It is not a software-licence identifier and does not inherit from a repository, site, collection, journey, author or rights holder. Native documents without an explicit declaration leave it absent. Gazette validates the URI without fetching its terms or treating the declaration as publication permission. The supported URI spelling is ASCII, with valid percent escapes and no credentials, whitespace or unsafe delimiter characters; query and fragment spelling is retained.

The field belongs to `ContentNode`, is charged to the existing graph text budget, and reaches the borrowed Tera/page, search and graph projections. JSON-LD emits the declared URI as `license: {"@id": "..."}`. The SQLite registry retains the same optional field; it does not resolve or infer a licence. Source-owner rights evidence and publication-policy admission remain separate requirements.

## Authored citation and downloads

To add a citation and downloads to a publication, follow [Supply a citation for the article](https://rawthoughts.in/gazette/credit-and-reuse/#supply-a-citation-for-the-article). The fields below define the contract for integrations and themes.

Optional `content.citation` carries plain `text`, ordered `formats` with plain `label` and `text`, and optional `bibtex` and `pdf` destinations. Any one may be supplied; an empty atom, blank text, duplicate format labels or unsupported control characters fail validation. Values count against existing graph text and record bounds. There is no inherited citation, generated citation style, article-subtype requirement or scholarly-status assertion.

```toml
[content.citation]
text = "Field Notes. Keep the context. https://notes.example/notes/keep-the-context/"
formats = [{ label = "Short citation", text = "Field Notes, Keep the context." }]
bibtex = "reference.bib"
pdf = "paper.pdf"
```

The top-level page `citation` borrows text and formats and resolves downloads once through the same source confinement, retained-only exclusions and serving URLs as body links. Local downloads must name a retained file of the declared format; `.bib` joins PDF as opaque static transport. Citation-only files enter selected-build asset closure and the page's `assets`. Explicit public HTTP(S) or root-relative destinations are retained without fetching or certifying availability. Errors identify the original frontmatter field, not a fabricated Markdown-body occurrence. Each present download also exposes `bibtex_destination` or `pdf_destination` using the same native `LinkDestination` contract as body links: `kind`, optional `content`, `site` and `presentation`, and `relationships`. These facts describe that resolved URL relative to the page's owning site; they do not change the URL or assert that an external file is available. A retained local download is a `resource`; an explicit URL uses the existing native resolver.

Templates must escape citation text. Direct and discovery-safe page/search projections retain the same resolved view. JSON-LD deliberately does not translate this into Schema.org `citation`, which means works cited by the page. Mirrors remain redirect envelopes; Markdown alternates retain their existing selected-metadata scope. The registry's complete semantic digest includes authored citation values, but its current normalized SQLite rows do not store this atom. This bounded reading-contract change does not alter the journal schema or upgrade existing stores.

## Explicit body summaries

A complete Markdown HTML event containing `<!-- more -->` selects the body prefix as an excerpt. Whitespace around `more` is permitted. A marker written inside inline/fenced code is literal text; a substring of a larger HTML event is not accepted. No marker means no summary. A leading marker means an explicit empty summary, not an automatic description or first-paragraph fallback.

The retained AST has one cached boundary and one anchor allocation. A second marker, or a marker inside a footnote definition or image alt text, fails with source coordinates. Paragraph, list, blockquote and other enclosing Markdown tags are closed in the prefix. Footnote references/definitions and heading IDs are omitted from the summary, so embedding an excerpt does not introduce broken footnote machinery or duplicate full-page heading IDs. The full body retains them. Its marker becomes an inert continuation span whose collision-free ID is allocated after every heading in the same anchor namespace.

`ParsedMarkdown::render_resolving_references` returns `RenderedMarkdown` with `body_html` and optional `summary`. Both projections share one complete-document safety check and one retained reference mapping. Invalid HTML, unsafe URLs, invalid anchors or failed link resolution after the cut still fail the entire operation. Excerpts are neither reparsed nor extracted by stripping HTML. Body HTML, summary HTML, summary text and mapped destinations have independent Markdown output ceilings. Parser events, depth and input have their own limits.

The page model exposes `summary.html`, `summary.text`, `summary.marker` (original body byte coordinates) and `summary.continuation`. Search borrows only the text as `summary`; JSON-LD uses it as `abstract`. The authored `content.description` remains independent. The marker is absent from body/search text, while syntax inspection records its range separately from untrusted raw HTML.

For a routed publication, source-reference validation happens before URL normalization. The compiler then qualifies the resolved link against that page's configured canonical URL once, for both body and excerpt. Continuation uses the same base. Thus an excerpt embedded elsewhere still links to its source page; templates never resolve source paths or make live requests. Standalone Markdown or a local view without a route keeps relative references instead of inventing a base. Qualification does not certify external destinations, fetch assets or change source ownership. The qualified continuation and complete page model remain bounded by configuration and consumer context/output limits.

This applies to framed authored content and configured plain native Markdown. Summary selection does not select a listing or change publication permissions. Themes, search, JSON-LD and applicable [feed formats](feeds.md) consume the resulting HTML or text through their own projections.

## Connected local page rendering

`ResolvedAuthored::project_page` derives one typed view for an exact authored object: an article, documentation page, other leaf family, collection or journey landing page. Its body comes from the retained Markdown AST. Node, site and project values are borrowed from the resolved graph/catalog, not read from SQL or rebuilt by templates. No lifecycle filtering occurs in this local inspection operation.

The view distinguishes explicit collections, journey participation and primary flags, ordered journey steps, direct collection children and outgoing editorial targets. `projects` contains the resolver's final page relationships and all contributing origins. `project_declarations` separately preserves the selected object's original declarations, including container `context`; context is not relabeled as a container's `about` claim. The site view supplies public home-site coordinates: its exact identity and publisher, optional publication metadata, rights holder, configured default author, explicit site roles and effective Theme/palette. It excludes private composition. A configured default author reaches the byline only under the published-content rule above; a publisher reference supplies neither authorship nor approval. The source path/digest remains typed evidence but is excluded from model serialization.

The SDK's compiled Theme exposes `compiled.render_page(&resolved_authored, &content_id, &template_entry)`. The application rejects different workspace digests or a different content home site **before** body rendering. It serializes the typed projection under the compiled context-byte ceiling, adds only selected Theme/palette identifiers, and gives that exact model to the rendering port. Template choice remains explicit; no filename, kind or missing field manufactures a fallback.

The returned `RenderedPage` keeps complete UTF-8 HTML, the exact model consumed by Tera, and separate private input/output evidence. Its model digest hashes compact `serde_json` serialization of `model()`; its HTML digest hashes `html().as_bytes()`. These are local read-back identities, not release-signature canonicalization. The shared bounded writer is also used by Tera, so context/output ceilings have one implementation. Failures return no successful partial page/model.

This is not a public admission path: draft/archived content and private project states remain visible deliberately. No author, rights, canonical URL or disclosure permission is invented. The Rust SDK supports selected-page rendering, and the [local publication command](local-rendering.md) connects retained content and project documents to complete isolated site output without Pale.

## What the model leaves to the publisher {#remaining-boundaries}

[Explicit routing](routing.md) binds brand publication bases and authored primary/alias paths once. The page projection borrows the compiler's canonical URL/output record; collision checks include cross-site public addresses. Routes remain local unadmitted values. Local site assembly can emit explicitly selected Theme alias pages; that is not evidence of HTTP redirects or public certification.

The model resolves configured meaning; rights, privacy and publication policy remain owner decisions. A cross-publication mirror, translation or syndication needs its exact declared relationship, rather than a match between names or routes. Extension references do not admit arbitrary schemas. Repository API reference uses the separate [documentation target](mintlify.md); it does not become authored publication content. [Registry state](store.md#the-registry) and append-only release history have different persistence responsibilities.

## Derived recommendations are not editorial declarations

Native page contexts expose bounded `recommendations` ranked from eligible resolved relationships and authored topical metadata. This generated value never mutates the authored `related` graph, assigns a new parent, changes a journey's order or upgrades a Mentions assertion to About. It is not accepted in authored frontmatter. The versioned [Theme contract](theme-reference.md#ranked-recommendations) defines eligibility, scores, shallow destinations and limits; every native interface consumes that same compiler path.
