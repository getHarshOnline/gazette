# Choose and read workspace sources

Bind a project to source files, check which files Gazette will read, and inspect the parsed documents before you build.

## Add a project's documentation {#example}

Extend `workspace-example.toml` from [the first example](workspace.md#check-a-complete-small-workspace) with the declarations below. They place one project's documentation on the existing `site:notes`; the principal, sites and brand already declared there stay unchanged.

```toml
[[projects]]
id = "example:project"
name = "Field Notes tooling"
lifecycle = "done"
visibility = "public"
availability = "gated"

[[sources]]
id = "example:docs"
location = { kind = "filesystem", root = "/absolute/path/to/field-notes-tool/docs" }

[[placements]]
role = 'primary'
site = "site:notes"
project = "example:project"
render = true
discoverable = false
documents = { source = "example:docs", target = "docs/project" }
```

Replace the absolute source root with your documentation directory, then check the complete file:

```sh
gazette check --stdin < workspace-example.toml
```

This checks the declarations without reading the source. Continue with [source acquisition](#acquire-pinned-documentation-for-a-site) to read it. The filesystem source observes current local files. For a revision-pinned repository source, select `kind = "repository"`, its repository `root`, the full lowercase commit ID and a confined `subdirectory`; use the repository's actual checked-out commit.

`role` is required. Every project that has any publication placement has exactly one `primary` placement and may have zero or more `mirror` placements. The primary is the project's canonical documentation home; a mirror is another publication placement for that same project. This edge neither creates a syndicated content copy nor a route alias/redirect, and it is unrelated to `projects.origin` (`original`, `successor-fork` or `convenience-mirror`). Missing, unknown, all-mirror and multiple-primary declarations fail complete workspace resolution.

A source `root` is a typed portable selection: `.` means the selected workspace root, a confined relative path means a descendant of that root, and an absolute path deliberately selects an external native root. Relative roots are resolved once by the application after the workspace itself has been selected; they never consult the process current directory during compilation. The logical spelling, not the observed absolute location, participates in configuration identity.

A local authored tree that is not a Git repository uses the distinct filesystem source kind. `subdirectory` is optional and, when present, is a confined literal selection beneath its resolved source root:

```toml
[[sources]]
id = "example:authored"
location = { kind = "filesystem", root = "sources/authored", subdirectory = "content" }
```

Acquisition retains one bounded complete tree and its exact bytes. The resulting tree identity is observation evidence only: it is not an atomic external snapshot, repository revision, admission, rights, permission or release approval. Repository and parent-gitlink sources continue to require their exact pins; failure never falls back between source kinds.

The example deliberately combines a completed project, public visibility and gated availability. These axes are independent. Placement controls are also explicit independent selections, not evidence that materialization or discovery is permitted. `deployable` belongs to the whole site, not a project placement, and does not authorize release. No source, database, output directory or remote is opened by this command.

`projects.name` is an optional explicit display value, preserved exactly. Blank or control-containing names fail; omission emits no name and never substitutes the identity key, repository folder or a page title. Equal names do not join distinct project identities. The same catalog value reaches Theme/page data, search, graph, JSON-LD and the private registry, including name changes/removal through complete-set replacement. This is local declaration/projection, not owner identity or public-field approval, and it does not delegate the field to an external fact provider.

`projects.repository` is an optional, explicitly published repository address, independent of acquisition roots, Git remotes and source pins. Workspace resolution requires an absolute HTTP(S) URL with a host and no credentials, whitespace, control characters or backslashes. Accepted spelling is retained exactly rather than normalized into another identity. Omission stays absent; compilation never contacts the address or substitutes an observed `origin` remote. The same project record supplies Theme, page/search/graph and private registry data; JSON-LD projects it as `codeRepository`. It is not a deploy destination, software licence, fork classification, reachability result or public-field approval.

`projects.license` is an optional owner-evidenced SPDX expression for the project's software at the selected source revision. Gazette validates strict SPDX syntax and retains the exact admitted expression; it does not canonicalize lax aliases, derive a licence URL, inspect repository files or infer a value from the repository address, source capture, package manifest or content. Omission stays absent/held. The same typed project value reaches Theme/page data, search, graph, JSON-LD and the private registry. It is never a content licence, chain-of- title decision, public admission or permission to publish.

`projects.origin` declares project lineage independently from names, addresses, state, ownership and client relationships. It is optional: omission means unspecified, not original work. The closed forms are:

```toml
# In the relevant [[projects]] declaration, select exactly one form:
origin = { kind = "original" }
# origin = { kind = "successor-fork", upstream = "project:upstream" }
# origin = { kind = "convenience-mirror", upstream = "project:upstream" }
```

An upstream must resolve to a distinct project in the same catalog. Cycles and unknown/wrong-kind endpoints fail complete workspace resolution; names and remote URLs cannot repair them. The upstream scalar participates in existing editor definition/reference navigation and source-origin inspection. A lineage edge does not create a placement, content membership, publisher role, rights grant, or a client engagement. The same declaration supplies page/search/graph data, JSON-LD and private registry projection; public discovery cannot expose a private or unlisted upstream through this edge. Public upstream identity references do not automatically export that project's full record or require hosted docs.

Syndication selections also belong to the brand publication, not to a renderer; see [feed configuration and identity](feeds.md#explicit-publication-configuration). Developer-manual and generated code/API-reference selection also belongs to the brand's existing `documentation_repository.build`: exact project modules, explicit manual page/asset selections, reference inputs, source bindings and target options are resolved there once. Its separate local acquisition/assembly path does not turn manuals into authored-document placements or publication `ContentNode`s. See [configured documentation assembly](mintlify.md#configured-local-assembly).

## Bind named roots on each machine

Use a named root when the same workspace reads content or writes output in different locations on different machines. Select the machine-binding file in the root Gazette document, before its tables:

```toml
machine_binding = "tools/studio.local.toml"

[[sources]]
id = "example:manual"
location = { kind = "included-filesystem", root = "machine:documents/Project", include = ["docs"] }
```

The selected binding supplies its `roots` table. It can also contain settings for other tools; Gazette reads only this table:

```toml
[roots]
documents = "/absolute/content"
repositories = "/absolute/repositories"
builds = "../generated"
```

Absolute values keep their native meaning. Relative values start at the binding file's parent directory, so `../generated` above selects the workspace's `generated` directory. Gazette does not expand environment variables or home-directory markers, search for another binding, or fall back to the process current directory. Root names start with an ASCII letter, contain letters, digits, `_` or `-`, and have at most 64 bytes. A `machine:name` coordinate selects the bound root itself; its optional `/path` uses the same confined-path rules as other relative selections.

The same coordinates work in publication `output.directory` and `documentation_repository.path`, such as `machine:builds/notes` and `machine:builds/notes-doc`. The binding selects paths; it grants no read, write, execution or publication permission. Keep machine-specific values in the existing private binding file, outside committed publication configuration.

Native workspace loading captures the selected binding alongside the root and included documents, then resolves paths from those retained bytes. The binding is not an included configuration module and cannot be selected as one; only the root document may choose it. Its exact bytes remain in acquisition evidence, while the portable declarations determine logical configuration identity. A missing or invalid binding stops workspace loading; using an unbound root stops the operation that needs it. Supplied TOML bytes alone cannot resolve named roots: load the workspace from an explicit native root to use them.

The native `LoadedWorkspace` result includes its selected `native_root` and `source_roots`, a map from configured source keys to resolved native roots before each source's subdirectory selection. CLI, MCP and SDK consumers use this projection instead of interpreting root spellings themselves. Unbound roots are absent, so an unused source does not prevent loading the workspace. This private projection is neither source acquisition nor proof of existence, branch or revision. Derived contexts must match the native workspace root and check the complete configuration inventory, including the binding, before reusing these paths. Moving identical configuration files requires a new native load; the logical configuration digest alone does not identify machine-specific inputs.

## Select filesystem inputs

For a manual source, name the files and directories it may read:

```toml
[[sources]]
id = "source:manual"
location = { kind = "included-filesystem", root = "tool", include = ["docs", "README.md"] }
```

`include` takes one to 128 unique literal paths relative to the source root. Each path must exist with that exact spelling, including letter case. A file includes only itself; a directory includes its complete tree, including hidden files. Gazette retains necessary parent directories and keeps paths relative to the root, so these inputs arrive as `docs/…` and `README.md`. Adding a sibling folder does not expand the capture. Missing selections, symlinks, unsupported files and exceeded size or depth limits stop acquisition. The declared includes remain in generated-source provenance.

Includes select current local bytes, including held edits. Add `expected_branch = "main"` to the location when the read must happen on a particular local branch. Gazette checks the branch at the exact repository worktree root before and after capture. A different branch, detached or unborn HEAD, invalid branch name or missing repository stops acquisition; Gazette never checks out a branch or changes Git state. Omitting the field keeps ordinary filesystem capture, with no Git requirement.

The branch check does not pin a commit or establish that working bytes match committed content. Use a repository source when the request requires revision verification. The direct SDK placement format uses `kind = "included_filesystem"` with the same `root`, `include` and optional `expected_branch` fields. Generated-source provenance retains the includes and the checked branch requirement.

### Exclude named paths {#explicit-filesystem-exclusions}

`filesystem` continues to capture the complete selected root. For a worktree that intentionally omits administrative or replaced files, use a distinct contract:

```toml
[[sources]]
id = "source:theme"
location = { kind = "filtered-filesystem", root = "/absolute/theme", exclude = [".git", "tokens/world.css"] }
```

`exclude` contains one to 128 unique, validated relative entry paths. An excluded directory excludes its descendants. Exclusions apply before inspecting or opening that entry; all other files, including dotfiles, retain ordinary capture semantics. The selection and exact exclusion list remain in source provenance. Repository and submodule capture are unchanged. When a theme resource is mounted from a design input, its package-local counterpart must be absent from that selected capture; Gazette still rejects ambiguous local-plus-mounted origins.

## Acquire pinned documentation for a site

Run `gazette check --stdin --acquire-site site:notes < workspace-example.toml` to read the sources for that site's rendered project placements. Gazette first resolves the declarations and checks for overlapping targets. It then checks source confinement, the pinned Git objects and aggregate limits before returning the captured inventory. A failure leaves no successful subset. The command skips disabled placements and placements on other sites. It does not fetch, check out, write, stage, open a database or approve publication.

Keep the report private. It names the configured roots, site and verification for each target. In Rust, `build::acquire_site` also keeps the immutable document bytes so later compiler phases can use that same capture. `--max-placements`, `--max-acquired-entries` and `--max-acquired-bytes` limit all acquisitions for the site. Per-source limits still apply.

Use `kind = "submodule"` with `root`, `parent_commit`, `submodule_path` and `subdirectory` instead of a repository `commit` when the parent pins the child. Gazette reads the revision from the parent's gitlink. It fails if the child is missing and never switches to a sibling clone. A workspace-relative root uses forward slashes and has no traversal, repeated separators, controls or platform prefix. An external root must be absolute and cannot contain dot components or repeated separators. Gazette does not expand home-directory markers.

### Require every enabled source {#materialization-obligation-dependency}

Gazette represents two source-selection states only:

- `render = false` deliberately disables that project-document placement and does not acquire it; and
- `render = true` requires an explicit `documents` binding, and every selected source must be captured and verified as part of the complete set.

`discoverable` remains independent and cannot weaken acquisition. There is no generic `required`/`optional` field on `ContentBinding`; an attempted `obligation` field is rejected instead of being silently ignored. Consequently a missing selected source is a hard acquisition failure and never reuses stale output. Project state, visibility, availability and discoverability do not make a selected source optional.

## Parse the captured project documents {#parse-retained-documentation}

Run this after choosing the workspace and site:

```sh
gazette check --stdin --acquire-site site:notes --parse-documents < workspace-example.toml
```

In Rust, call `acquired.parse_documents(corpus::Limits::default())`. The returned `ParsedSite` borrows the acquisition and keeps the parsed trees for later resolution. It reuses those bytes rather than reading changed files.

Gazette accounts for every regular file under its placement target and relative source path. Empty directories remain in the inventory.

| Input | How Gazette parses it |
| --- | --- |
| `.md` or `.markdown` (case-insensitive) | Markdown. A first line of `+++` starts strict TOML frontmatter. Malformed framing or TOML fails the parse. This inspection adds no title, author, publisher or lifecycle to plain Markdown. A later build can apply a configured `placements.native_markdown` profile. |
| `.landing.yaml` or `.landing.yml` | [Structured landing](content-model.md#structured-landing-sources). Other YAML is opaque and does not provide Markdown frontmatter. |
| `.ipynb` | The [notebook syntax profile](notebooks.md). Gazette rejects invalid notebooks without fixing IDs, guessing roles or treating them as downloads. |
| Other files | Opaque assets. The inventory records byte identities even for files that are not valid text. |

The report lists parsed and opaque inputs in path order. It can include private source information. It is not public search or a rendered site. Raw HTML can appear in parsed syntax and still fail the HTML-fragment renderer. Links do not imply collection, journey, ownership or other relationships.

`--max-corpus-files`, `--max-corpus-bytes`, `--max-corpus-nodes` and `--max-corpus-metadata-nodes` limit the entire corpus. Per-document and aggregate path limits also apply. Duplicate input and whole-set limit failures stop before parsing. A document error stops at the first failing logical path, without a partial success. For semantic checks and templates, continue to [local rendering](local-rendering.md).

## Resolution rules

Publication content resolves against the declared sites, projects, principals and taxonomies. Publication bindings select which source bytes enter that resolution; source paths do not supply editorial relationships.

### Bind content to the workspace

The Rust API's `Compilation::resolve_content` consumes the existing typed content declaration and returns a `ResolvedContent` only after both structural relationships and every content home-site reference resolve. It borrows this exact compilation, including the source digest, instead of copying a site catalog into each renderer or database projection. `site_for` resolves an exact content identity to its declared home site; a matching principal or project key is not a substitute site.

Draft and archived objects are checked too. Cross-site relationships keep their explicit homes; a site's `deployable=false` does not make its structure invalid. There is no permission inference or lifecycle filtering. This is an internal structural phase, not a new frontmatter format, a CLI build or publication admission. See the [content model](content-model.md) for the current API and remaining seams.

## Publication acquisition

`build::acquire_publication` captures the consumer's presentation sources, including mounted resource sources in its selected Theme/dependency closure, and all configured authored and enabled project-document sources once per exact source key under one aggregate budget. `Workspace::content_bindings()` is the shared role-aware view used by acquisition and mapping; `content_source()` resolves a logical entry back to that same declaration without guessing from public URLs. Other publications participate in graph resolution, not in the selected site's primary output. Presentation metadata checks only presentation-role sources. `parse_publication` borrows captured bytes; no source scan or hidden reread occurs. The [local rendering path](local-rendering.md) additionally verifies that each object's declared home matches the publication that owns its source binding. The source's project identity is provenance, separate from the document's explicit about/mentions, collections and journeys. Binding data does not imply authorship, rights, or public admission. Native coordinates remain private acquisition evidence.

[Back to workspace](workspace.md).
