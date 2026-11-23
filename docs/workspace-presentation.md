# Compose themes and site resources

Attach a Theme, choose providers and shared resources, and set presentation for a single page or the whole site.

| If you want to… | Read |
| --- | --- |
| Attach a Theme and declare its package sources | [Theme and package composition](#theme-plugin-capability-provider-and-resource-composition) |
| Use a shared file for a Theme resource | [Supply a declared Theme input](#supply-declared-theme-inputs-from-a-shared-source) |
| Compile a Theme to check templates | [Compile templates](#compile-selected-retained-templates) |
| Override the template for one page | [Per-page presentation](#per-content-presentation-selection) |
| Change navigation and utility copy | [Site chrome and control copy](#site-chrome-and-control-copy) |

## Declare Themes, plugins and browser resources {#theme-plugin-capability-provider-and-resource-composition}

Workspace-level `[composition]` declares reusable defaults. Each site's `composition` declares only its own selected changes. The compiler first resolves the workspace and site choices. After acquiring the selected Theme packages, `check_metadata` adds their resource defaults beneath those choices. Metadata checks and rendering use that completed composition; workspace-only editor queries show the declarations available before package acquisition.

```toml
# Root tables must precede the example's [[sites]] and other array tables.
[composition]
theme = { package = "theme:base", palette = "palette:light" }

[composition.plugins]
"search-plugin" = { state = "enabled" }

[composition.capabilities]
"publication.search/v1" = { state = "enabled", provider = { kind = "plugin", package = "search-plugin", provider = "search" }, requirement = "optional", allow_absence = "no-native-acquisition" }
"publication.analytics/v1" = { state = "disabled" }

[composition.resources]
"module:search" = { state = "enabled", resource = { owner = { kind = "plugin", package = "search-plugin" }, resource = "module:search" }, output = "/assets/search.js", use = "site" }
```

Theme and plugin packages have separate bindings to configured `sources` using explicit filesystem, exact repository or parent-gitlink coordinates. Paths and pins are not repeated in Theme, capability or resource selections. A repository source must name its actual checked-out commit before acquisition can succeed.

Inside a site declaration, for example:

```toml
[sites.composition]
theme = { package = "theme:research" }

[sites.composition.plugins]
"search-plugin" = { state = "disabled" }

[sites.composition.capabilities]
"publication.search/v1" = { state = "enabled", provider = { kind = "gazette", provider = "static-search" }, requirement = "required" }

[sites.composition.resources]
"module:search" = { state = "disabled" }
```

The examples show declaration shape, not a complete production configuration; all referenced source keys require explicit definitions. Resolution rules are:

- Omitted Theme, plugin, capability and resource atoms inherit independently. An empty map means no local changes; it does not erase workspace selections.
- A site Theme replaces the **entire** Theme binding. Its omitted palette means no named palette; another Theme's palette is never carried across implicitly.
- Each capability entry replaces the whole selected binding, including its failure policy. Partial provider records cannot merge into a valid selection.
- Each plugin entry activates or disables one package as a whole atom. Package activation is independent from its catalog/source binding and from every provider or resource it exports; neither export reference activates it.
- Each resource entry independently replaces its package-qualified resource, output and `site`/`dependency` use as one atom. It does not activate or prove a capability.
- Explicit `state = "disabled"` remains in the result and cannot fall through to workspace or Theme discovery. Extraneous fields on disabled entries fail.
- `ProviderRef::Gazette` selects a Gazette-owned implementation. `ProviderRef::Plugin` selects one provider exported by one exact captured plugin package. Neither form is a contribution scope or inheritance origin.
- Resource ownership is separately package-qualified as Theme or plugin. A Theme never chooses the provider or performs acquisition.
- Source references in **all** package bindings are checked, including defaults shadowed by every site. Missing sources, owners and misspelled variants fail rather than discovering a substitute.
- Required/optional expresses selected policy only. Optional does not authorize arbitrary degradation or provider substitution. Source reads require the explicit acquisition operation below. Provider execution, resource compilation and certification remain separate later phases.

Per-site expansion is bounded before cloning each retained selection. Caller TOML `nodes` independently caps total resolved Theme/plugin/capability/resource entries; caller `source_bytes` independently caps their logical site, selection, package, provider, source and palette key bytes. These are separate from input parsing budgets. Exceeding either yields `workspace_limit`, never a partial site set. The source digest retains exact bytes; equivalent declarations can yield identical resolved values with different source digests.

### Let a Theme supply its resources

A Theme can declare each resource's default output beside its inputs and recipe:

```toml
[[resources]]
id = "stylesheet:world"
inputs = ["styles/world.scss"]
build = { kind = "stylesheet", entry = "styles/world.scss" }
output = "/assets/world.css"
use = "site"
```

`output` enables this default when the Theme is selected. `use` defaults to `site`; `dependency` is available for browser modules reached through another module. A declaration without `output` still requires an explicit workspace selection. The output must be a file route, and `use` cannot be supplied without an output.

Defaults follow the selected Theme's dependency graph. A dependent Theme can replace an inherited resource by declaring its own output for the same resource ID. Shared ancestors reached through two branches contribute one declaration; competing declarations from independent branches require an explicit declaration in their dependent Theme. Each resolved default retains its declaring package.

Workspace and site resource selections override these defaults as whole entries. An explicit `state = "disabled"` blocks the default; disabling a resource named in `resource_requirements` fails admission. `metadata.inspect().composition` shows the effective values and their `theme`, `workspace` or `site` origins. Emitted resources and their URLs come from that same composition.

### Acquire the effective composition

```sh
gazette check --stdin --acquire-composition example:site < workspace.toml
```

For a native modular workspace, select the root and exact configuration file:

```sh
gazette check --workspace-root /absolute/workspace --workspace-file gazette.toml --acquire-composition example:site
```

The same selectors work with `--publication` and its review or registry projection. Native selection rejects simultaneous TOML on standard input. It uses the shared workspace loader, retaining include-file evidence and resolving relative source roots without depending on the process directory. Detached standard-input TOML keeps its original semantics.

This operation compiles the whole workspace before reading the effective Theme package graph and the explicitly enabled plugin packages. Capability-provider and resource references must point into that admitted package set; they never activate a package implicitly. It captures each exact source key once, including shared resource inputs mounted by selected Theme packages and their admitted dependency closure, preserving all consuming selections and their origins in the compiled configuration. Different keys are not merged just because paths or commits match. Disabled, shadowed and other-site sources are not opened; their configuration references still must be valid. Each selected repository or parent-pinned submodule uses the same confined, exact-pin reader as documentation acquisition.

The Rust SDK's `build::acquire_composition` returns an `AcquiredInputs` with retained bytes keyed by source identity, not invented documentation placements. Its `inspect()` projection matches CLI JSON and includes configuration and native verification but not file payloads. This is private operator evidence: configured paths remain present, so do not publish the report. Later native changes cannot alter the retained capture; a new acquisition performs new verification.

`--max-placements` bounds unique source keys for this operation before any native read. `--max-acquired-entries` and `--max-acquired-bytes` bound aggregate retained inventory, tightening each native capture to the remaining allowance. Per-source limits remain enforced, including a separate per-source Git metadata allowance. All selected sources must succeed, even if a consuming capability is optional; failure returns no successful subset, fallback or implicit degradation.

The operation cannot be combined with documentation acquisition/parsing or origin queries. It writes nothing, executes no provider, accesses no network and does not certify a build. An empty effective composition returns an empty capture, not a usable Theme. Capture alone does not verify Theme/plugin/provider identity, named palette membership or compatibility.

### Check captured package declarations

Each Theme or plugin package binding explicitly names its manifest and captured source. There is no default filename, scan or lookup in another repository:

```toml
# This root table precedes array tables in the workspace document.
[design_inputs]
tokens = "example:tokens"

[[sources]]
id = "example:theme"
location = { kind = "filesystem", root = "/absolute/path/to/theme/publication" }

[[sources]]
id = "example:tokens"
location = { kind = "filesystem", root = "design/tokens" }

[[theme_packages]]
package = "theme:base"
source = "example:theme"
manifest = "theme.toml"
resource_inputs = { "shared/_tokens.scss" = { input = "tokens", entry = "dist/scss/_tokens.scss" } }

[[plugin_packages]]
package = "search-plugin"
source = "example:theme"
manifest = "search-plugin.toml"
```

Replace the absolute Theme root with your source directory before acquisition. Use a revision-pinned repository source when the build must bind a particular commit. Manifest paths and package-local resource paths are relative to the captured subtree. A Theme manifest owns templates, render entries, palettes, components and Theme dependencies. A plugin manifest independently exports providers and resources:

```toml
package = "search-plugin"

[providers.search]
implements = ["publication.search/v1"]

[resources."module:search"]
inputs = ["scripts/search.js"]
build = { kind = "browser-module", entry = "scripts/search.js" }
```

Providers and resources are separate exact-key maps. One package may export both; that does not make a provider own or produce the static resource.

### Supply declared Theme inputs from a shared source

`design_inputs` maps workspace-local aliases to existing source keys. Names match `[a-z][a-z0-9_-]{0,63}` exactly; they are not package identities, filesystem paths or capability selections. Every source and alias reference must resolve even when its package is unselected. An alias alone neither selects a package or resource nor permits a native read.

The example's `theme_packages.resource_inputs` supplies the Theme's logical `shared/_tokens.scss` input from the retained `dist/scss/_tokens.scss` entry of `example:tokens`. Add the mapping to the existing package record, not a second record with the same identity. Both paths are literal confined `EntryPath` values. The logical target must already appear in a resource's `inputs` within that package's own manifest, for example:

```toml
# Resource declaration inside the existing theme.toml, not workspace frontmatter.
[[resources]]
id = "style:main"
inputs = ["styles/main.scss", "shared/_tokens.scss"]
build = { kind = "stylesheet", entry = "styles/main.scss" }
```

Here `styles/main.scss` remains package-local and can import `../shared/tokens` through the existing Sass input resolver. The manifest may supply its output and use as a resource default, or the workspace may select them through `composition.resources`. The input alias alone does not emit a stylesheet. Package-local inputs need no mapping. Mounted inputs are supported for Theme resource bindings, not plugin bindings, page companions or Tera templates.

Acquisition captures each selected source key once under the existing source-count, entry, byte and revision budgets. Shared aliases to one key do not duplicate that capture. Only selected Theme packages and their admitted dependencies bring their mounted sources into acquisition; unused aliases and unrelated packages do not trigger a directory scan. Native reads still require the caller's explicit permission and the configured source's normal confinement and pin checks.

Metadata rejects undeclared logical targets, absent or nonregular retained entries, and any target that exists both in the Theme capture and in a mount. Compilation assembles an exact virtual resource-input tree from those retained bytes and uses the existing resource adapters; it does not copy inputs into the Theme repository or grant the compiler live source access. Retained replay uses the same captured bytes, even if native files subsequently change.

Private resource evidence records the original alias, source key, entry, source-binding digest, retained tree identity and file digest. Public resource URLs and Theme resource records retain their existing shape without these coordinates. See [resource compilation](page-resources.md#theme-resources-can-use-retained-shared-inputs) for the page-resource boundary.

Mounted `dot-image` resources use the same explicit diagram acquisition as package-local DOT: exactly one declared input, the configured pinned `dot_renderer` executable and separate execution permission. Acquisition resolves the mount to its original retained source and entry, invokes the provider and retains the checked PNG bound to those exact bytes. Later rendering verifies that retained binding and projects the PNG into the resource's virtual input/dependency namespace without rerunning the provider or reading native files. Original source provenance and virtual resource paths remain distinct. An executable pin does not attest Graphviz's shared libraries, plugins, fonts or ambient dependencies.

### Check the package manifests {#validate-the-complete-selected-package-set}

```sh
gazette check --stdin --acquire-composition example:site --check-composition < workspace.toml
```

The Rust equivalent is `acquired.check_metadata(toml::Limits::default())`. It borrows retained source bytes and parses each selected manifest once using the shared TOML parser. `metadata.inspect()` returns the acquisition evidence and Theme/plugin package maps keyed by their canonical package identities, with exact source bindings, manifest paths, digests and parsed declarations. Theme entries also expose retained resource-input origins. This is private operator output, not suitable for public publication; it is not a public manifest-AST or byte-coordinate accessor.

Complete manifests reject unknown fields, duplicate set entries, empty provider capability lists and invalid resource recipes, even in unselected declarations. Effective selection then requires:

- The exact selected Theme and its optional declared palette name.
- A nonempty retained template directory.
- Each selected provider to be exported by its exact Gazette or plugin owner and declare the exact requested capability.
- Each selected resource to be exported by its exact Theme or plugin package.
- Every selected resource input to exist as a retained regular file, either package-local or explicitly mounted through its Theme binding.

Palette/provider/capability/resource/input lists express sets, not execution order. The typed declaration sorts them for deterministic projection and bounded lookup; the retained AST preserves their original order and exact source coordinates. Several selected capabilities may share one provider. Resources are admitted and compiled independently, so this never implies a shared output.

Independent providers do not need to be Theme-owned. Disabled selections are not checked as active providers or resources. Optional capability policy cannot silently skip broken package metadata or resource inputs. An absent Theme fails this check; an empty acquisition is not sufficient. Failures return no successful metadata subset or fallback.

CLI `--max-source-bytes` and `--max-nodes` independently bound configuration parsing and the aggregate selected manifest set; `--max-depth` applies per file. The SDK accepts those metadata limits separately from acquisition. The operation performs no new native reads, writes, provider execution, rendering or publication. Metadata checking validates declared package members and their selected inputs. Resource compilation, provider execution and Theme rendering check their behavior in subsequent operations.

### Compile the configured templates {#compile-selected-retained-templates}

```sh
gazette check --stdin --acquire-composition example:site --compile-templates < workspace.toml
```

This performs the same complete metadata check, then compiles only the effective Theme's explicitly declared `templates` subtree. It is mutually exclusive with `--check-composition`, document parsing and origin queries. No native source is reopened after acquisition and no files are generated. CLI compilation uses the shared default rendering ceilings; SDK callers can supply explicit `render::Limits` to `build::compile_templates(&metadata, limits)`.

Every regular file inside each admitted package's selected template subtree is parsed, including nonstandard suffixes; invalid unused templates fail compilation. Gazette strips that package's template-directory prefix and mounts the retained name beneath its declared virtual namespace. Includes, inheritance and root entry selection therefore use `<namespace>/<subtree-relative-name>`, never an unqualified subtree-relative name. Metadata, capability inputs and sibling directories outside the declared template roots are not templates. The SDK returns an owned compiled Theme that can render supplied presentation context after the retained acquisition is dropped or native files change.

The CLI returns the same private compilation evidence as the SDK: configuration digest, site/Theme/palette selection, source coordinates and full-tree identity, manifest path/digest, selected directory and template-tree identity. Native paths make this operator evidence, not public output. It proves neither palette behavior, capability execution nor admitted content-to-page projection. See the [rendering contract](tera.md) for escaping, bounded-output and trusted-template limits.

## Choose a template for one page {#per-content-presentation-selection}

The Theme manifest supplies defaults for the existing content structures and page families:

```toml
[render]
page = "page.html"
home = "home.html"
collection_directory = "collection-directory.html"
kinds = { article = "reading.html", collection = "index.html", journey = "sequence.html", project = "product.html", utility = "utility.html" }
modes = { "mode:home" = "home.html" }
```

`kinds` accepts `article`, `documentation`, `project`, `index`, `utility`, `extension`, `collection` and `journey`, directly matching the compiler's existing content model. `documentation` means a page explicitly declared with that family; it does not turn product help into a separate family. Help retains its authored article or collection structure and documentation purpose. `modes` uses the exact mode already declared by an extension page's `presentation`, without deriving it from a title, path or site. The engine does not prescribe a Theme's mode names.

`home` selects the exact content named by the publication's `home`. `collection_directory` selects an index whose listing declares `root-collections`. These roles come from declared relationships, independently of page titles and routes. An authored project catalogue without a distinct declared role keeps its explicit page template.

Kind, mode, home and collection-directory entries inherit through declared Theme dependencies and are qualified with the namespace of the package that supplied them. A dependent package can override an entry; competing independent providers require an explicit choice. All selected entries must exist in the captured template namespace. An explicit page `template` wins, followed by its workspace binding, its declared mode, its home or collection-directory role, its kind, and finally `render.page`. Listing continuations follow the same choice as their index.

Select a retained page template without putting a world-specific path into authored Content:

```toml
[sites.composition.page_templates]
"content:product-introduction" = "example/product.html"
"content:publication-about" = "example/about.html"
```

The table belongs to that exact site's composition. Omission inherits the complete workspace map; `page_templates = {}` explicitly clears it, and a nonempty site map replaces the default map atomically. Every key must resolve to content owned by that site. Each entry must name a regular file in the admitted virtual Theme namespace. There is no filename, route, title, wildcard or folder-based selection.

The selection does not change content identity, editorial membership, route, author, or source provenance. Its original configuration document is available through composition-origin inspection with `field = {kind = "page-templates"}` and the content key in the remaining field path.

## Site chrome and control copy

`composition.chrome` supplies skip-link, navigation, footer, search and settings labels. Each label inherits independently: an explicit site value wins over the workspace value, which wins over the selected Theme’s declared default. The same rule applies to each namespaced `regions` key. Empty strings are rejected rather than treated as omission.

A Theme declares its defaults under `[chrome]` in its manifest, using the same copy fields as the workspace. Dependencies supply omitted labels; a dependent package may override individual labels. Conflicting declarations on separate dependency branches require an explicit override. Gazette keeps each winning package or workspace/site origin. It supplies no built-in English copy.

For example, a site can change one label while inheriting the other controls:

```toml
[sites.composition.chrome.shell.settings]
title = "Reading preferences"
```

Workspace compilation resolves workspace and site declarations. Captured Theme admission completes their defaults; inspection, review, component rendering and page rendering use that completed composition. Rendering requires complete labels for the visible controls. Workspace origin queries resolve each explicit workspace or site label’s retained source span; a table assembled from several origins has no single source span. Admitted Theme labels retain their declaring package in the composition’s origin map.

The ordered destinations are separate: `composition.navigation` and `composition.footer` each inherit when omitted, while an explicit empty site list clears only that list. Changing the menu therefore does not replace shared chrome copy. These fields control presentation; they do not activate a capability or release a publication.

An entry may supply an optional namespaced `key` alongside `label` and `destination`. Keys are unique within each navigation or footer list and survive resolution unchanged. They let a Theme address one configured destination by meaning without matching visible wording or a content/site identifier. Gazette assigns a key no layout or product meaning; omission preserves an ordinary ordered entry.

The optional `regions` map carries bounded presentation copy under owner-namespaced keys. Gazette validates and transports at most 256 effective regions in stable order; the Theme assigns their layout, component and visual priority.

Before Theme rendering, every navigation destination is resolved through the selected content graph and canonical route table. Cross-site destinations retain their actual home site and optional Theme/palette presentation. Search copy may also name one `no_script_destination`; it must resolve to an existing routed page, so Gazette never fabricates fallback HTML or derives a destination from a label. The no-script destination inherits only from an explicit workspace or site declaration. Themes supply labels, not content destinations, reading handoff endpoints or control selections. Set `shell.search.enabled = false` in a workspace or site chrome declaration to suppress inherited search copy; a higher-layer explicit `true` can restore it. Copy alone never selects the search index or its provider.

```toml
[composition]
navigation = [
  { label = "Home", destination = "content:home" },
  { label = "Notes", destination = "content:notes" },
]
footer = [
  { label = "Privacy", destination = "content:privacy" },
]

[composition.chrome]
skip_to_content_label = "Skip to content"
navigation_label = "Primary navigation"
footer_label = "Site footer"

[composition.chrome.regions]
'region:preface' = "A deliberate opening note"

[composition.chrome.shell]
close_label = "Close"

[composition.chrome.shell.search]
trigger_label = "Search"
title = "Search this site"
field_label = "Query"
loading_message = "Loading results"
empty_message = "No results"
unavailable_message = "Search is unavailable"
no_script_destination = "content:search"

[composition.chrome.shell.settings]
trigger_label = "Settings"
title = "Display settings"
color_mode_label = "Color mode"
color_mode_system_label = "System"
color_mode_light_label = "Light"
color_mode_dark_label = "Dark"
motion_label = "Motion"
motion_system_label = "System"
motion_reduce_label = "Reduce"
motion_allow_label = "Allow"
```

Optional `composition.chrome.reading_assistance` declares user-invoked external reading handoffs as typed data, separately from search/settings and generic region copy. The Theme receives the same object at `presentation.chrome.reading_assistance`; omission remains absent. For example, appended to the chrome declaration above:

```toml
[composition.chrome.reading_assistance]
label = "Ask about this passage"
prompt_template = "Explain {selection} from {article_title} ({article_url}) on {site_name} ({site_url}). Section: {section_title}. Context: {context}"
selection_limit = 4096
context_limit = 1024

[[composition.chrome.reading_assistance.providers]]
id = "assistant:chosen"
label = "Ask a chosen assistant"
endpoint = "https://assistant.example/new"
query_parameter = "prompt"
```

This example endpoint demonstrates shape, not a working service. The composing workspace chooses the actual providers and their supported prefill endpoints; Gazette embeds no provider registry or endpoint fallback. Providers retain declared order and unique namespaced IDs. Endpoints must be explicit HTTPS addresses without credentials or fragments. They cannot already contain the selected prompt query key, including its percent-encoded spelling. `query_parameter` matches `[A-Za-z][A-Za-z0-9_-]*`, with at most 64 ASCII characters.

Prompt templates accept only the single-brace slots `selection`, `article_title`, `article_url`, `site_name`, `site_url`, `section_title` and `context`; `selection`, `article_url`, `site_name` and `site_url` are required. Unknown or unbalanced braces fail validation. Templates allow newlines and tabs, but no other control characters. The template and selection limit each cap at 4096 Unicode characters; selection must be positive. Context caps at 1024 Unicode characters, with zero explicitly excluding it. Labels cap at 256 Unicode characters; endpoints cap at 2048. One to eight providers are permitted. Reading assistance keeps its existing whole-object selection: a site with no chrome declaration inherits the workspace’s assistance, while a site with its own chrome declaration must explicitly supply the complete assistance object to retain handoffs. Omitting it from that local declaration suppresses the inherited handoffs. This selection does not merge endpoints or prompts label by label. Origin inspection retains nested source spans, and provider occurrences and text bytes are charged before site expansion clones the object.

The Theme may offer a reader action that inserts the supplied page/site values, bounds the selected text and nearby context, and URL-encodes the prompt. It opens the configured destination only after the reader chooses that action. Gazette validates the settings without filling prompts or making requests; the configured endpoint still needs to support the intended prefill behavior. These settings are presentation data, not content metadata or a JSON-LD relationship.

## Select page invitations

`composition.invitations` selects where a Theme may offer a link below a page. Each namespaced key inherits independently from the workspace; a site's entry replaces the complete policy. Footer entries and template names do not select an invitation.

```toml
[sites.composition.invitations."theme:services"]
state = "enabled"
destination = "content:services"
articles = true
pages = { "content:about" = true, "content:personal-essay" = false }
```

`articles` includes article-family pages, including those within collections. Exact `pages` entries override that default: `true` selects the page and `false` excludes it. To select only named pages, set `articles = false`. An explicit `state = "disabled"` suppresses the inherited invitation on the whole site. Omission inherits; an empty map makes no changes. Selection does not change a page's lifecycle, authorship or publication approval.

The destination must resolve to existing routed content. Each page override must name routed content in the selecting publication, including collection and project landing pages and excluded entries; missing and foreign references fail rather than disappearing silently. Selected pages expose `invitations["theme:services"].destination`, using the same resolved `content` and `route` fields as navigation. Preview URLs follow the destination site's preview route. Unselected pages omit the map, and generated views have no authored-page invitation. `presentation.chrome.invitation_destinations` exposes the configured targets by semantic key independently of those visible selections, so a destination can receive handoff context without inviting itself or requiring a footer link. The Theme supplies appearance and copy and can suppress a repeated invitation already present in the body.

## Project resource destinations

Optional project `resources` declare independent navigation roles. The existing `repository` remains repository metadata; `resources.github` instead declares a GitHub navigation endpoint when no repository is established. Both together are refused, so there is one configured source of the GitHub action. These fields select no acquisition, deployment, public availability or implementation state.

```toml
[[projects]]
id = "project:example"
name = "Example"
lifecycle = "planned"
visibility = "public"
availability = "unavailable"
repository = "https://code.example.test/team/example"
resources = { landing = "content:example", code_documentation = "https://docs.example.test/example/", learn = "content:example-guide", mentions = "content:example-mentions" }
```

`landing` names the project's canonical introduction by exact content identity. It must be a routed, canonical project-family page or root collection with a direct About declaration for that project. A root collection's About describes the collection itself, not its children.

Pages receive optional `projects[].landing` with `id`, `title`, serving `url` and source-relative `destination` values. Learn, Mentions and repository addresses do not fill a missing landing. Selecting this link adds no About/Mentions assertion or project-ownership relationship.

A `project-articles` listing may omit its legacy `landing` field when the project owns this resource. When both are declared they must agree; without either, listing resolution fails. This gives every article view the same exact landing without copying it into each selector or inferring it from another page.

Each resource field is optional and has a specific destination contract:

| Field | Destination |
| --- | --- |
| `code_documentation` | An explicit public HTTPS address for separate developer manuals/reference. |
| `learn` | An existing documentation-family page or this project's explicitly selected root-collection landing. Another collection is not an implicit fallback. |
| `mentions` | A `project-articles` index selecting the same project. `residence = "all"` combines standalone and collected articles. |
| `blogs`, `thoughts` | Independently configured article views using that same selection, ordering and pagination mechanism. |

The compiler resolves serving URLs and destination classification once. URL validation rejects credentials and malformed addresses without contacting a host. The action order is code-documentation, Learn, GitHub, Mentions, then separately configured article views. A missing resource stays absent; the Theme can show its unavailable state without substituting a broad site or organisation index.

A project-family host with exactly one directly declared `about` project receives the same shallow resource projection in its page model and hosted-component context. A root collection receives that projection only when it is the selected landing for its single About project and satisfies the same canonical-route checks. Ambiguous or inherited relations do not choose a project. The Theme supplies visible labels and action hierarchy from composing chrome. Native Markdown alternates lower the same component formatter, preserving these anchors; SQLite retains each optional declaration independently. JSON-LD keeps `codeRepository` confined to the actual `repository` field and represents learning/article/collected-article/manual resources as `subjectOf` references, not hosted-state claims.

[Back to workspace](workspace.md).
