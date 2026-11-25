# Configure publication identity and delivery

Set who publishes, what the site includes, and where its site or developer docs can be delivered.

| If you want to… | Read |
| --- | --- |
| Display the name, website or contact of a person or organisation | [Principal display names](#principal-display-names), [principal websites](#principal-websites-and-identity-references) and [contact links](#contact-projection-dependency) |
| Choose who publishes or operates a site | [Independent site responsibilities](#independent-site-responsibilities) |
| Bind content, legal copy or a documentation mount | [Brand publication settings](#brand-publication-settings) |
| Point a developer docs build or release at its own destination | [Documentation repository settings](#developer-manual-and-generated-reference-repository-settings) and [publication destination](#publication-destination) |

## Principal display names

A principal may supply `name` beside its `id` and `kind`. This is an explicit display value, not an identity join or evidence of legal identity, consent or publication approval. Present names must contain a non-whitespace character and no control characters; accepted spelling and spacing are preserved. Different principal keys may have the same name. Omission stays absent, with no key, email, brand or publisher fallback.

Content attribution borrows the selected principal from this catalog. It does not maintain a separate author-name schema; see [direct attribution](content-model.md#direct-attribution-not-identity-invention).

### Principal websites and identity references

The principal can explicitly select its website, independently from any site it publishes or any contact interface:

```toml
[[principals]]
id = "person:writer"
kind = "person"
name = "A writer"
website = { kind = "site", site = "site:personal" }
same_as = ["https://writer.example/"]
```

A `site` website names a declared publication with a public base URL. Its resolved `website.url` follows the selected public or validated local-preview address; `website.canonical_url` retains the configured public address for identity data. An outside website uses `website = { kind = "external", url = "https://writer.example/" }`; both resolved addresses then retain that explicit HTTP(S) value. Native `website.destination` carries the same link-destination shape as body links: kind, optional exact target site, relationships and destination presentation. Typed site websites classify relative to the owning rendered page or listing; explicit outside websites remain external without hostname inference.

`same_as` is an ordered list of explicit public HTTP(S) identity references. Declared owned accounts add their resolved URLs to the owning principal after those entries, without duplicates. Duplicate entries, credentials, whitespace and unsafe URL schemes are rejected. These declarations express the owner's selected identity links; Gazette performs no remote identity verification. Without an explicit identity reference or owned account, identity links stay absent. A publisher role, display name, email or displayed social profile establishes no ownership. Page attributions and generated listing items expose the same resolved website and principal data. A Theme can link an author's name with `rel="author"` without rejoining the catalog or embedding a publication-specific address.

### Configure a contact link {#contact-projection-dependency}

Gazette has no generic principal contact field. In particular, `contact_email` and arbitrary contact maps are rejected as unknown fields rather than being accepted as identity, copied into public output or hidden in an extension bag.

Publication contact interfaces are a separate, supported contract: the brand publication may declare `contacts.correspondence`, `contacts.rights` and `contacts.discovery`. These explicit addresses are available through the shared publication metadata, without joining them to a principal or verifying a mailbox. They neither establish identity nor imply legal responsibility. Publication profiles select workspace accounts through `accounts`, or retain inline `social` values. Displaying a profile supplies no principal identity claim; only an explicit account `owner` adds its URL to that principal’s `same_as` links.

Principal-owned contacts are not part of the current schema. Use publication contacts for those interfaces rather than adding an unrecognized principal field.

## Declare accounts once and select them for display

Keep each public account in the workspace registry, then select its ID wherever a publication should display it:

```toml
[[accounts]]
id = "account:writer-github"
platform = "github"
handle = "writer"
owner = "person:writer"

[[accounts]]
id = "account:shared-video"
platform = "youtube"
handle = "@SharedChannel"

[[accounts]]
id = "account:writer-linkedin"
platform = "linkedin"
url = "https://www.linkedin.com/in/writer/"
owner = "person:writer"
```

The owning principal must already be declared. Omit `owner` while ownership is unresolved; a publisher, operator or administrator is not an inferred owner. The account can still be displayed. An owned account contributes its URL to that principal’s identity links even when none of the publications displays it. Gazette does not verify account control remotely.

Inside an existing site with its own `name`, or an existing `[[brands.publications]]` table, select the accounts:

```toml
accounts = ["account:writer-github", "account:shared-video"]
```

The same IDs may be selected by several sites. Workspace and publisher defaults may also select `accounts`; a nearer list replaces the entire inherited list, and `accounts = []` clears it. Each publication can select one account per platform. Duplicate IDs, duplicate platform-and-URL pairs, unknown owners or selections, repeated selections and collisions with an inline `social` platform are errors.

An explicit public HTTP(S) `url` wins. When it is absent, `github`, `x` and `youtube` derive their standard profile URL from `handle`, accepting one optional leading `@`. Other platforms, including LinkedIn, require an explicit URL. Handles and optional `channel_id` retain platform metadata; neither grants access nor enables an integration. Derivation rejects path separators, query or fragment syntax and empty handles.

Selected accounts supply the existing `site.metadata.social` profiles, so Themes and discovery use the same resolved metadata as inline profiles. `gazette check --why social.youtube.url --site site:notes` reports the registry declaration that supplied the value, pointing to the handle when the URL was derived. Query `accounts` to inspect where the publication’s selection came from. Editor navigation follows account selections and their declared owners across included configuration documents.

## Independent site responsibilities

Sites may also select `rights_holder` and `default_author` principal keys:

```toml
# Inside an existing [[sites]] table; all keys must name declared principals.
rights_holder = "example:rights-holder"
default_author = "example:writer"
```

The rights holder may be a person or organisation; `default_author` must name a person. A published article with no attributions credits that configured default author. Any explicit attribution, including a rights-holder-only declaration, keeps its own meaning and prevents this fallback. Draft and archived objects receive no default author. The resolved graph supplies the same credit to the byline, JSON-LD and other consumers. Selecting a default author is a publication policy choice; it does not turn an agent's draft into that person's published work.

`rights_holder` remains independent site context and supplies no missing content rights claim or licence. Neither field inherits from a publisher or sibling site.

Each site may declare `roles` alongside its separate `publisher` reference:

```toml
# Inside an existing [[sites]] table; these principal keys must be declared.
roles = [
  { principal = "example:canon-authority", role = "canon-authority" },
  { principal = "example:controller", role = "data-controller" },
  { principal = "example:operator", role = "site-operator" },
  { principal = "example:processor", role = "data-processor" },
]
```

The core `PublicationRoleBinding` supplies one exact principal/role pair. The compiler validates every principal and rejects duplicate pairs; it normalizes the set without choosing a primary holder. Multiple principals may have the same role, and one principal may explicitly have several roles. Omission means no declaration, not inheritance from a publisher, another site or workspace defaults. Original TOML positions remain available for editor navigation after normalization.

Page and search data carry the same borrowed principals under `site.roles`. Site/discovery graphs retain the bindings and only their referenced principals; role edges consume the graph budget. JSON-LD uses the existing technical `gazette:siteRoles` vocabulary with an explicit site reference, not invented Schema.org properties or author/affiliation aliases. Standard feed authors remain content authors. The complete private registry receives these same bindings.

Role bindings describe the declared responsibilities. They do not create page authorship or establish the authority, consent or publication approval behind a role.

## Brand publication settings

### A site with its own brand

This complete configuration keeps the publication beside its site identity:

```toml
[[principals]]
id = "person:writer"
kind = "person"
name = "A writer"

[[sites]]
id = "site:notes"
name = "Field notes"
publisher = "person:writer"
deployable = false
domain = "notes.example"
port = 8080
language = "en"
contacts = { correspondence = "mail@notes.example" }
```

`domain` is a hostname and selects its HTTPS root. Use `public_base_url` instead when the publication has a deployment prefix or another explicit address. `port` is the short form of `local_port`; neither spelling starts a listener. Declaring both spellings of one setting is an error. `theme = "theme:example"` selects an already configured Theme package; a table may also select its palette. Use either that short form or `composition.theme`.

The `name` makes this site its own brand, with the stable generated brand key `brand:<site-id>`. Publication fields such as `authored`, `machine`, `processing`, `legal` and `documentation_repository` belong directly in this site block. Their contracts are the same as in the full brand-publication form below. A second publication binding for the same site is an error.

To keep an existing brand key or declare a parent, add its identity beside the site's `name`:

```toml
# Inside the [[sites]] table above.
brand = { id = "brand:notes", parent = "brand:journal" }
```

Here `brand:journal` must be declared elsewhere, using either form. Omit `parent` for an independent brand. The inline table declares the brand; it does not refer to or merge with another declaration of the same ID. It requires the site's `name`, and duplicate brands, missing parents and parent cycles remain errors. Brand parentage supplies no publisher, rights holder or author defaults.

Use `owner` for a declared principal, `parent` for a brand above this brand, `subject` for the project or work that the brand represents, and `projects` for its portfolio. A research programme may have its own brand and its own members. A product can be in the brand's portfolio and in the programme's roadmap. The optional `description` explains the brand's role. In the full form, `identity` gives a brand an explicit public IRI when it has no home or shares an identity across publications.

Set the publication's `about` to a routed same-site `AboutPage` to name the brand as its main entity. Reserve this binding for a page with no other authored primary subject. It requires a declared home. A personal profile or a page about a research programme can continue naming its own subject.

Without `brand`, `site:notes` still creates `brand:site:notes` with no parent. Keep the full form for several publications under one brand. Editor navigation follows inline IDs and parent references to their original source; brand field origins find `id` and `parent` in the inline table and `name` beside it. Use publication-origin queries for the effective publication fields and their defaults.

### Shared publication settings

Declare shared values once and override only the fields that differ:

```toml
[defaults.publication]
language = "en"
machine = { sitemap = true, json_ld = true }

[[publishers]]
id = "person:writer"
publication = { contacts = { rights = "rights@notes.example" } }
```

The publisher ID must name an existing principal. A publication receives workspace defaults, then the settings of its site's declared publisher, then its own fields. This works with both site forms and with explicit included configuration files. Supported shared fields are `language`, `contacts`, `accounts`, `social`, `machine` and `processing`. Identity, sources, routes, legal selections and chapter order remain explicit.

Tables merge field by field; scalars and lists replace the inherited value. An explicit `false` or empty list therefore remains an override. An empty table contributes no fields. Defaults are checked even when unused, and the resulting publication must satisfy the complete schema. Publisher settings do not reach another publisher's sites through a common brand, source or Theme.

From a workspace directory, inspect the winning value and its original document and field:

```sh
gazette check --why contacts.rights --site site:notes
```

Use `--workspace-root /absolute/workspace` for an explicit local workspace, or supply a complete single-document configuration on stdin. The result identifies the site, publisher or workspace default that supplied the field, its source span and its effective value. Query a scalar or list; a merged table can have several origins. This reads configuration without building or publishing a site.

### A brand with several publications

The publication's Theme-rendered authored sources are selected here independently of the developer-manual/code-reference documentation destination:

```toml
[[brands]]
id = "example:brand"
name = "Example Public Brand"
[[brands.publications]]
site = "example:site"
public_base_url = "https://example.test/"
home = "content:home"
language = "en"
contacts = { correspondence = "hello@example.test", rights = "rights@example.test" }
social = { github = { url = "https://github.com/example", handle = "example" } }
authored = [{ source = "example:authored", target = "publications/example" }]
documentation_repository = { name = "chosen-domain-doc", path = "/absolute/generated/domain-doc", destination = { kind = "git", remote = "https://example.invalid/chosen-domain-doc.git", branch = "published" } }
machine = { sitemap = true, search = 'lexical', json_ld = true, markdown_alternates = { enabled = true, directory_page = 'index.md', file_page = 'append-md', media_type = 'text/markdown', discovery_rel = 'alternate' }, llms = { enabled = true, description_from = 'content:home', representation = 'markdown-alternate', include_human_link = true, sections = [{ title = 'Start here', contents = ['content:home'] }] } }
identity_assets = { site_icon = "image:site-icon", link_preview = "image:link-preview" }
```

`authored` is an array of bindings; each `source` references an existing explicit `[[sources]]` declaration. Declaration order is retained. Select owner-held roots explicitly rather than copying their pages into one central source. The singular table form is not accepted. `target` is a logical corpus prefix, not a URL, output folder or inferred project identity. The same `ContentBinding` shape serves project-documentation placements; the owning field supplies its role. Equal or nested prefixes across authored sources and enabled project-document placements fail complete configuration resolution before any native reads. Disabled placements do not reserve a corpus prefix.

To give readers a short route through a publication, set `machine.llms.coverage = "navigation"`. Gazette uses the configured site navigation order and takes titles and descriptions from the selected content. A site with assembled developer manuals also gets a **Developer docs** section with the admitted project guide links and the provider index. Use `curated` with explicit section contents when the introductory path needs a different selection. `all-pages` still supplies a complete reading catalogue. Sitemap, search and feeds have independent selections.

An existing authored or project-document `ContentBinding` may also name `retained_only`, an exact set of source-relative opaque files kept solely as input evidence:

```toml
# Replace the existing binding, not the publication or source declaration.
authored = [{ source = "example:authored", target = "publications/example", retained_only = ["metadata.bin", "guide/metadata.bin"] }]
```

Every listed entry must be a captured regular opaque file. Its bytes, inventory entry and provenance remain unchanged, but it has no content, page-companion, asset or output obligation. The disposition stays visible in private inspection. Absent paths, directories, Markdown, notebooks, duplicate paths and conflicting dispositions for the same source are refused. This is not a basename, dotfile, glob or directory-ignore rule; unlisted unknown inputs still fail publication. Relative links, declared resources and program/notebook dependencies cannot use retained-only bytes. Mounted Theme resource inputs and native tools whose exact selected source is a full-tree dependency cannot consume them either.

An optional brand `parent` names another declared brand. Missing parents and cycles fail resolution; parentage supplies no publisher, controller, contact, language or author inheritance. A publication's optional `home` names a routed same-site content object in its selected graph. Building one site does not require acquiring another site's home. Excluding the selected home refuses the affected publication rather than silently choosing another page.

Publication `language` is a normalized BCP 47 tag and supplies the language of content that omits its own. An explicit content language wins. Contact roles remain independent; after shared settings are resolved, omitted values stay absent. Named `social` profiles require an explicit public HTTP(S) URL; optional `handle` and `channel_id` retain supplied platform metadata without enabling an integration. Pages, discovery views and Theme contexts borrow one resolved `site.metadata` view. It exposes the serving home URL, including an explicit preview prefix, but no native source paths or private deployment coordinates.

The optional `static_assets = { route_prefix = "/media/example/" }` inside that same binding selects a directory destination for supported local byte transport. It applies equally to authored sources and project-document placements. There is no independent asset-path setting in Theme or an adapter. See the [static-asset contract](render-inputs.md#static-assets-from-configured-sources) for supported suffixes, source-relative references, shared model fields, bounds and the distinction between copied bytes and public/capability admission.

## Compose owner-held legal copy without duplicating it

A brand publication may select exact owner-held legal entries through `legal`. Each binding names the retained source identity separately from the composed page's identity and route. Add the following fields inside an existing `[[brands.publications]]` table whose site is already declared:

```toml
legal = [
  { source = "example:legal", entry = "terms.md", source_content = "content:owner-terms", id = "content:site-terms", route = "/terms/", family = "utility" },
]
```

`example:legal` must have its own ordinary `[[sources]]` declaration. Its selected `terms.md` uses a reduced TOML-frontmatter profile, not a site-assigned authored page declaration:

```text
+++
[content]
id = "content:owner-terms"
title = "Terms"
description = "The owner's reusable terms."
lifecycle = "draft"
language = "en"
+++
# Terms

The owner's retained legal prose.
```

Only `id`, `title`, `lifecycle` and optional `description` and `language` belong to that source profile. Site, route, structure, authorship and other page fields are refused. `source_content` must match the exact source declaration. Gazette retains its body and original coordinates while producing the explicitly named Utility page; it does not rewrite the owner source or create a corporate site. Several publications may reuse one source with distinct output IDs and routes. Source and output identity collisions fail rather than silently choosing a copy.

A binding may also list `aliases`, released old addresses on the same site that redirect to the composed page. They have the same meaning and checks as an authored route's [aliases](routing.md): each is a site-relative route path, `"/tnc-policy/"` writes `tnc-policy/index.html`, and self-aliases, duplicates and collisions with any other route fail.

```toml
legal = [
  { source = "example:legal", entry = "terms.md", source_content = "content:owner-terms", id = "content:site-terms", route = "/terms/", aliases = ["/tnc-policy/"], family = "utility" },
]
```

The composed page receives the publication's shared publisher, roles and public contacts. Its source declaration, entry and digest remain separate from the output identity in private evidence. The publisher must still establish the notice's authorship, legal authority and approval; composing it supplies none of those decisions.

## Processing choices describe intent, not running services

The same publication may declare `processing` independently of its legal source bindings and site roles:

```toml
# Inside the existing [[brands.publications]] table.
processing = { hosting = { provider = "github-pages" }, analytics = { enabled = false, provider = "google-analytics" }, subscription = { enabled = false, provider = "google-apps-script" }, discussion = { enabled = false, provider = "giscus" }, webmcp = { enabled = false } }
```

Hosting names the intended provider; it does not prove deployment. Explicit disabled choices remain visible in private inspection and never activate a service, acquire its credentials or invent a public endpoint. Composed legal pages receive `legal_processing` with the selected hosting and explicit feature switches. Disabled provider details stay out of that public context; publisher, roles and contacts remain in the one shared `site` view.

Enabled analytics requires an explicit `measurement_id` and independently selected `publication.analytics/v1` capability. Enabled browser WebMCP requires explicit `tools` and the independently selected `publication.webmcp/v1` capability; its `search` tool also requires the publication's existing machine search selection. These public client bindings freeze before rendering, without contacting Google or a browser. They load or register nothing automatically. See [browser capabilities](browser-capabilities.md) for exact configuration and Theme composition. Absent or disabled processing conflicts with an enabled corresponding capability instead of silently hiding it. Enabled subscriptions remain unsupported and refuse with `workspace_processing_unsupported` before source acquisition. WebMCP's browser tools are separate from Gazette's local MCP interface. Enabled Giscus requires the existing selected native discussion capability and its site or rendered-project repository binding; an optional `repository` must match that binding exactly. Disabled discussion conflicts with an active native discussion selection rather than merely hiding it from the legal page. Unknown provider fields and identities are refused. These declarations do not establish service activity, privacy compliance, consent or publication approval.

## Set defaults for project Markdown {#plain-native-document-defaults}

An optional field on the existing project placement selects plain Markdown lowering:

```toml
[[placements]]
role = 'primary'
site = "site:notes"
project = "example:project"
render = true
discoverable = false
documents = { source = "example:docs", target = "native/example" }
native_markdown = { route_prefix = "/docs/example/", lifecycle = "published", language = "en-US" }
```

This replaces the corresponding example placement; do not append a duplicate site/project pair. The profile requires the document binding, an explicit directory route ending in `/`, and a content lifecycle. It applies only to unframed project Markdown, never to publication-authored input or explicit TOML frontmatter. Disabled placement sources remain excluded. The compiler uses the same shared source binding, corpus, graph and route resolver, not a separate native-doc publication model.

The optional `language` uses the same validated, case-normalized tag type as authored content. It supplies a default only to these unframed native documents; framed content does not inherit it, even when its own language is omitted. When the native profile omits language, the publication's configured language can supply it. If both omit it, language remains absent. Neither filenames nor programming languages select one. This profile supplies no taxonomy memberships or classification inheritance.

The [native Markdown contract](render-inputs.md#plain-project-native-markdown) describes derived coordinate identities, H1/filename title evidence and README/index route selection. Source-relative Markdown links resolve against retained targets; missing/escaping targets and unsupported fragments fail. No collection, authorship, about/mentions or public approval is inferred. Selecting `published` is an explicit default state, not certification or release permission.

## Mount project manuals on an existing site {#mount-native-documentation-beneath-an-existing-publication}

`documentation.mounts` selects Theme-rendered native documentation without inventing a project record or a Mintlify destination. It belongs to the workspace root, independently of `brands.publications.documentation_repository`:

```toml
[[documentation.mounts]]
site = "example:site"
landing = "content:home"
name = "Product documentation"
source = "example:docs"
route_prefix = "/manual/"
render = true
discoverable = false
lowering = { lifecycle = "published", language = "en-US" }
```

The site and source must already be declared. A rendered mount requires explicit `lowering.lifecycle`; optional language applies only to unframed Markdown, using the same native-document lowering and retained source-relative link resolver as project documentation. Its `landing` must resolve to an existing same-site routed content object in the selected graph. Theme receives `documentation_mount.name` and the shared resolved `documentation_mount.landing`, not private source paths. That association creates no editorial residence, project identity or semantic mention.

`render = false` validates and reports the declaration without acquiring its source, rendering pages or adding discovery output; `lowering` may then be omitted. `discoverable = true` requires rendering. Neither switch approves publication. This renders documentation through a publication Theme. For a separate developer site, use the [Mintlify documentation target](mintlify.md).

## Developer-manual and generated-reference repository settings

The developer-manual and generated code/API-reference target is selected in the brand's publication binding. Its name/path are not inferred from a domain and are not duplicated in the Mintlify adapter, project records or a second site-level setting. Each configured project module may select authored manual sections, generated reference sections or both in one ordered navigation model.

```toml
[[brands]]
id = "example:brand"
name = "Example Public Brand"

[[brands.publications]]
site = "example:site"
documentation_repository = { name = "chosen-domain-doc", path = "/absolute/generated/domain-doc" }
```

Set `documentation_repository` on the existing selected publication; the enclosing tables above show its placement, not additional brand or publication declarations. `workspace::compile` resolves the site link; `Workspace::documentation_repository` borrows that sole configured target. The repository name may differ from the local directory name. One brand may configure several publications, each with a separate target; conflicting site bindings or reused exact destination paths fail resolution. Missing settings mean no configured target, never an invented `domain_doc` directory.

The local `path` uses the same native root grammar as source and publication output roots: `.` selects the explicitly loaded workspace, a confined relative path selects its descendant, and an absolute path selects an external directory. Relative coordinates require retained native workspace acquisition and never use the process current directory. Detached supplied TOML can describe such a path, but cannot materialize it without that native coordinate. The destination must already exist; assembly and declaration alone never create it or grant writes.

An optional delivery selection can use the table form below. Replace an existing inline `documentation_repository` value rather than declaring both forms:

```toml
[brands.publications.documentation_repository]
name = "chosen-domain-doc"
path = "/absolute/generated/domain-doc"
destination = { kind = "git", remote = "https://example.invalid/chosen-domain-doc.git", branch = "published" }
```

This destination belongs to the complete documentation repository and never inherits the authored publication's destination. Omission is local-only. Exact duplicate remote/branch selections across authored and generated repositories, including two targets on one site, fail complete workspace resolution. Different branches remain different selected targets; Gazette does not infer whether a remote spelling aliases another native repository.

This declaration selects a destination and documentation inputs. Assembly checks target compatibility; native output operations separately check paths and require write permission. Configuration does not create a directory, Git repository or hosted site.

Select repository developer manuals and generated reference here. Product-user guides remain owner-held Content rendered as Gazette collections; commands and filenames do not determine that audience. A brand's publication binding does not establish publisher, rights or legal ownership.

## Publication destination

Delivery selection belongs alongside the brand's other publication settings:

```toml
[[brands.publications]]
site = "example:site"
destination = { kind = "git", remote = "https://example.invalid/publication.git", branch = "published" }
```

Add `destination` to the existing publication table rather than declaring that site twice. `Workspace::destination` borrows this sole selection. Omission means local-only operation; no remote, branch or provider is inferred. A Git delivery target is not a GitHub account or deployment-platform integration.

The compiler validates the exact remote text and literal branch grammar using the same rule as release authorization verification. It rejects unknown target kinds, malformed tables and duplicate exact remote/branch selections across sites. It does not normalize or resolve remote aliases, probe reachability or infer that differently spelled remotes are distinct native repositories.

Local preparation stores the authored selection in its immutable intent and includes it in the private output-policy input, not in public output. A declaration does not create a branch, configure Git, establish the remote pre-state, select credentials or grant permission to publish. Developer-manual/generated-reference repository settings remain a separate path and do not inherit this publication destination implicitly.

[Back to workspace](workspace.md).
