# Mintlify documentation target

Turn a Markdown guide from your code repository into a Mintlify page. Gazette checks links and assembles the pages, sidebar and generated API reference that Mintlify displays. Edit guides and API comments at their source, then assemble again. Keep product help in its publication and link to developer docs where the reader needs implementation detail.

## Add a manual to Field Notes {#configured-local-assembly}

Start with [the Field Notes workspace](https://rawthoughts.in/gazette/first-publication/). You will add one guide called **Notes tooling** at `/notes-tool`.

::::steps

:::step "Write the guide"

In a directory for the project's developer docs, save this as `index.md`:

```markdown
# Notes tooling

Use the note's stable identity when your integration updates its title or address.

[Return to the note](https://field-notes.example/keep-the-context/).
```

:::

:::step "Give the guide an address and a sidebar entry"

In `gazette.toml`, find the existing `[[brands.publications]]` entry with `site = 'site:field-notes'`. Add these tables before the next publication or top-level declaration:

```toml
[brands.publications.documentation_repository]
name = 'notes-code-docs'
path = '/absolute/path/to/notes-code-docs'
public_base_url = 'https://docs.field-notes.example/'

[brands.publications.documentation_repository.build]
target = { kind = 'mintlify', theme = 'mint', primary_color = '#315c4a' }

[[brands.publications.documentation_repository.build.modules]]
project = 'project:notes-tool'
slug = 'notes-tool'
title = 'Notes tooling'
manual = { source = 'source:notes-tool-manual' }
navigation = [
  { kind = 'manual', title = 'Build an integration', pages = [
    { title = 'Notes tooling', route = 'notes-tool', input = { kind = 'file', path = 'index.md' } },
  ] },
]
```

Add the project and source at the top level. Reuse their keys if you already declared them.

```toml
[[projects]]
id = 'project:notes-tool'
name = 'Notes tooling'
lifecycle = 'active'
visibility = 'public'
availability = 'unavailable'

[[sources]]
id = 'source:notes-tool-manual'
location = { kind = 'filesystem', root = '/absolute/path/to/notes-tool/docs' }
```

Set `root` to the absolute directory containing `index.md`. Set `documentation_repository.path` to the intended documentation repository. Preview assembly writes to a fresh directory and leaves that repository untouched. Use [documentation materialization](output-materialization.md#documentation-uses-the-same-writer-not-the-theme-renderer) to update it. TOML paths are literal: Gazette does not expand `$HOME` inside them. Write the full paths so Gazette reads the directories you intended.

> [!TIP]
>
> If a guide links to component READMEs or examples outside `docs/`, select the repository root and use repository-relative input paths, such as `docs/index.md`. Navigation selects the pages, and their resource links select companions; you do not need a second list of files to capture.

Within the module, `input.path` finds the source file and `route` gives it a page address without a file extension. The `navigation` array sets the sidebar order. Add more modules to include other products' manuals, each with its own sources and URL prefix. Gazette assembles these modules into one documentation repository with one root `docs.json`. Connect that repository to one Mintlify [Site project](https://www.mintlify.com/docs/organize/projects).

To set the colors used in both appearance modes, add `light_color` and `dark_color` to `target`. Mintlify uses `light_color` for emphasis on dark surfaces and `dark_color` for buttons and hover states. Choose readable colors from your publication's palette; Gazette passes these explicit values through without inventing shades. Both are optional and accept `#RGB` or `#RRGGBB`, like `primary_color`.

Add `telemetry = false` to `target` to write Mintlify's `integrations.telemetry.enabled = false`, which also removes Mintlify's feedback widgets. `telemetry = true` writes the enabled form. Omit it to keep Mintlify's default and write no `integrations` block.

The optional `public_base_url` records the documentation site's public address. A publication's project resource view derives its Docs action from this base and the exact project's first manual navigation page, using the same route normalization as assembly. Omit the base while no public destination is selected; the local documentation build still works. A project can take its facts from an [authored landing](content-model.md#keep-project-facts-on-their-landing) instead of the complete inline declaration above. Checking or acquiring this manual uses the configured project identity and does not capture the landing's publication content.

:::

:::step "Check the settings and open the page"

From your Field Notes directory, check the single-file starter workspace:

```sh
"$GAZETTE" assemble-docs --check --site site:field-notes < gazette.toml
```

This checks the plan from TOML alone. The build checks the manual files and links. Continue at [Build and open the preview](https://rawthoughts.in/gazette/publish-developer-documentation/#build-and-open-the-preview) to run the build and open **Notes tooling** under **Build an integration**.

:::

::::

## Fix a missing page, heading or image {#keep-links-with-their-sources}

If an article links to `reading.md#examples`, list both articles in navigation. Gazette resolves the path from the source article, checks that the destination has the `examples` heading ID and writes a link to the configured page address. Gazette reads the selected pages and follows their relative image and resource links to capture the files they need. Explicit companion selections remain available when you need to choose their output paths.

When a guide includes an H1, put it first, use it once and match the title in navigation. Mintlify displays that title once. Gazette stops assembly if a required input is missing or a checked link is invalid. See [Authored developer manuals](mintlify-formats.md#authored-developer-manuals) for the input and link rules.

### Local preview links

Keep public URLs in the guides. When building for local preview, map the public base to the loopback address where the other preview is running. Include the mount path, if there is one. The CLI takes the map in `--preview-link-bases-json`. The Node SDK takes it in `previewLinkBasesJson`.

In Rust, use `Mintlify::new(&Comrak).with_preview_link_bases(Some(&bases))` and pass that target to `inputs.assemble_documentation(&target, limits)`. The convenience function `build::assemble_documentation` uses public URLs.

The map changes hyperlinks and preserves their path, query and fragment. Code examples and URLs outside the map keep their original text. Omit the map for files you intend to use at public addresses. For the return link from a product page and a worked two-server setup, see [Publish developer documentation](https://rawthoughts.in/gazette/publish-developer-documentation/) and the [preview address rules](preview.md#developer-local-documentation-links).

## Assemble documentation from Rust

Call the SDK when your application needs the generated files. Use a Gazette checkout with the [source-build prerequisites](install.md#what-a-build-needs) and add this dependency:

```toml
[dependencies]
gazette-sdk = { path = "/absolute/path/to/gazette/sdk/rust", features = ["native-local"] }
```

This function returns the assembled `docs.json`. Pass the absolute workspace directory, a manifest path relative to that directory such as `gazette.toml`, and the site key. The site must have documentation configured and its input files must be available.

```rust
use gazette_sdk::{build, configuration, source, workspace};
use std::{error::Error, path::Path};

fn documentation_config(
    root: &Path,
    document: &source::EntryPath,
    site: &workspace::Key,
) -> Result<String, Box<dyn Error>> {
    let loaded =
        configuration::load_workspace(root, document, Default::default())?;
    let inputs = build::acquire_documentation_workspace(
        loaded,
        site,
        build::PublicationSetAcquisitionLimits::default().sources,
    )?;
    let assembly =
        build::assemble_documentation(&inputs, source::Limits::default())?;

    let path: source::EntryPath = "docs.json".parse()?;
    let bytes = assembly
        .output()
        .file(&path)
        .ok_or("assembled target has no docs.json")?;
    Ok(std::str::from_utf8(bytes)?.to_owned())
}
```

`configuration::load_workspace` loads the manifest and its includes. If your host already has the complete workspace as TOML text, use `acquire_documentation`. With a modular workspace, loading the root file as text alone leaves out the includes.

The build report’s `unselected_manual_pages` lists eligible Markdown omitted from manual navigation, explicit assets and imported reference selections, grouped by source. Gazette finds those names through a bounded metadata scan without reading the omitted pages. Check `unselected_manual_pages_incomplete` too: it names sources whose scan failed or is unsupported, so an empty list for one of those sources does not mean every page was selected. Review the report when adding chapters; it does not change the workspace’s order.

Untracked files ignored by Git stay outside a working repository’s capture, and a required ignored link fails with its input path. Explicit includes and exclusions still constrain access. An `expected_branch` check still applies before and after capture. Pinned repository and submodule sources verify the selected bytes against their commits and mark the evidence `selected_only`; they do not certify the rest of the source tree. See the [source binding rules](workspace-sources.md) for roots and revision choices.

`acquire_documentation_workspace` captures the manual and reference files. Assembly borrows those bytes, so keep `inputs` alive while reading the result. Use `build::check_documentation` when you have a complete TOML buffer and only need to validate the plan.

Use `generate_documentation` followed by `assemble_generated_documentation` for reference produced by a configured command. See [Run a documentation generator](documentation-generators.md) for how to set up and run that command. The Node SDK returns a report and file buffers from [`assembleDocumentation`](../sdk/js/docs/native-api.md#documentation-repository-assembly).

## Read the assembled files {#how-the-build-fits-together}

Assembly returns the files in memory. List them with `assembly.output().inventory()` and read them with `assembly.output().file(&path)`.

| Look here | For |
| --- | --- |
| `docs.json` | Site appearance, project modules and sidebar order |
| `notes-tool.md` | The example guide at its configured route |
| Companion files | Images and other resources at their configured paths |
| Reference pages and artifacts | Ready-to-render pages or inputs to Mintlify's SDK and API conversion |
| `.mintignore` | Build inputs excluded from Mintlify's page discovery |
| `robots.txt`, `.well-known/tdmrep.json` | The owning publication's crawler rules and TDM reservation, when it reserves rights |

### Write the files to a directory {#durable-local-documentation-preparation}

Use `stage_documentation` to write a fresh directory for preview. To update an existing directory, use [documentation materialization](output-materialization.md#documentation-uses-the-same-writer-not-the-theme-renderer). `prepare_documentation` records a build in the journal using the assembled files, without rerunning generators. That repository has its own optional Git destination. It does not use the publication's branch by default. See [Releasing](releasing.md) for delivery operations.

## Choose an input format {#native-format-adaptation}

<a id="generated-markdown"></a> <a id="swagger-2-conversion"></a> <a id="sphinx-directory-packaging"></a> <a id="rustdoc-static-reference-projection"></a> <a id="openapi-http-reference"></a> <a id="schema-provenance"></a>

Use the [format reference](mintlify-formats.md) to match the files you have to the pages you want to build.

| You have | Gazette produces |
| --- | --- |
| [Written manuals](mintlify-formats.md#authored-developer-manuals) | Markdown pages at configured routes |
| [Generated Markdown](mintlify-formats.md#generated-markdown) | Markdown reference pages under the project and reference slugs |
| [TypeDoc JSON](mintlify-formats.md#typedoc-json) or [Sphinx JSON](mintlify-formats.md#sphinx-json-directories) | Inputs for Mintlify's SDK reference |
| [Rustdoc HTML](mintlify-formats.md#rustdoc-html-directories) | Static Rust API pages |
| [OpenAPI or Swagger](mintlify-formats.md#openapi-and-swagger) | Inputs for HTTP API reference |

See [bundled schemas](mintlify-formats.md#schema-provenance) for the schemas the adapter uses.

## Retain a validated page catalog

`DocumentationTarget::assemble_with_catalog` returns the same complete target files together with optional validated discovery evidence. Mintlify supplies the actual provider reading route, configured module, available title and readable text, known anchors, and the exact retained artifacts that establish each page. A native Rustdoc `.md` companion and its `.mdx` provider page share one entry. TypeDoc and OpenAPI expose their qualified root routes; the catalog does not invent symbol or operation addresses. Missing text or anchor evidence stays absent.

The native publication recipe automatically captures and assembles configured imported documentation before rendering. This uses the existing source acquisition and Mintlify adapter, including their confinement and complete-set bounds. `DocumentationAssembly::catalog_capture` can also supply an immutable result from the existing documentation operation through `PublicationRecipe::with_documentation_catalogs`. A supplied catalog must match the freshly captured documentation sources and exact configuration; it bypasses assembly only for that configured owner. Configured generated references without a supplied result or an explicit generation selection return `GenerationRequired` rather than disappearing from discovery.

For local rendering and synchronous preview, `documentation_generation` selects an existing absolute private staging parent and the existing configured generators. Native execution needs explicit permission; selecting a workspace generator alone grants neither staging nor execution. The CLI accepts `--documentation-generation-parent` with `--allow-execute`; MCP `render_sites` additionally requires its outer `allow_execute` flag. The SDK's `with_rendered_documentation` callback delivers original native job receipts and any exceptional custody before the operation returns. Complete-set job, repeated staged-input, generated-output and response bounds apply before the relevant effects. `render-sites` and preview accept `--max-total-source-entries` for an explicit complete acquisition and repeated staging entry allowance; omission preserves the native default. The refusal reports observed jobs, entries and bytes beside their limits. A later rendering or response-size failure retains completed receipts; inspect their stages before retrying. If CLI stdout closes, the original report is written to stderr with a failure exit.

Synchronous development preview reuses generated output only while its exact documentation source identities match. A failed unchanged input replays the original receipt instead of repeating native effects after an unrelated content edit. Background preview sessions refuse explicit documentation generation before starting a worker because they cannot yet retain exceptional native custody; use synchronous preview. Preparation and qualification keep their imported-or-supplied documentation path effect-free.

The application binds modules to their configured project, site and brand. A publication capture may contain more files than a manual's selected closure, but every shared documentation entry must retain its exact kind and bytes. Independent documentation source identities and protected roots remain attached after assembly. Development preview watches those sources too; an unavailable or changed supplied catalog produces an explicit failure while retaining the last complete generation. The private cache identity covers admitted text, titles, routes, anchors, incomplete roots, output, configuration and source evidence.

Published projects contribute these validated provider pages to site search and the all-pages `llms.txt` list without becoming authored content nodes. A missing provider title remains absent in documentation metadata; its exact reading route serves as the display label. Reading links use real provider routes, never guessed Markdown endpoints. Canonical addresses come from `documentation_repository.public_base_url`. An explicit documentation serving base can produce preview addresses without changing that canonical address. Existing outbound `preview_link_bases` may rewrite reading links when explicitly configured; that mapping does not establish or start a documentation listener.

Gazette writes the configured public base without a trailing slash as `seo.metatags.canonical` in `docs.json`. Mintlify [appends each page’s slash-led path](https://www.mintlify.com/docs/optimize/seo#set-a-canonical-url). An absent public base omits the setting. Preview links use their own addresses. Check the rendered canonical and `og:url` on the deployed documentation site.

When the owning publication declares [`terms`](deployment-output.md#terms-of-use) with `tdm_reservation = true`, the documentation host states the same policy as the site. `docs.json` adds `tdm-reservation = "1"` and `tdm-policy` with the publication's terms address to `seo.metatags`, so every page carries both tags. Gazette writes `.well-known/tdmrep.json` with the site's rule, citing the same terms, and adds `.mintignore` exceptions for it. If the publication's `robots` policy is enabled, a root `robots.txt` with the same groups replaces the file Mintlify would otherwise generate, which permits AI training; it names Mintlify's `sitemap.xml` under the documentation `public_base_url`. A reservation whose terms address cannot be resolved fails the assembly. Without a reservation, Mintlify's own `robots.txt` stays in place even when the site declares robots rules. Mintlify documents a root `robots.txt` and `seo.metatags`, but not serving a dot-directory, so treat the meta tags as the reliable signal and check `/.well-known/tdmrep.json` on the deployed host.

Catalog validation checks every registered page and its exact artifact paths, including pages with no incoming link. Page, anchor and aggregate byte limits refuse the complete catalog when exceeded. Incomplete generator roots remain separate from usable entries; an adapter without catalog support returns `None`, not an empty catalog claiming completeness. Configured generation still requires the existing explicit generation operation. A catalog proves retained local evidence, not a hosted build or permission to publish.

## Extend the adapter

Implement the two methods on [`DocumentationTarget`](../crates/gazette-ports/src/documentation.rs). `preflight` checks the plan without opening source files or running a generator. `assemble` takes the captured manuals and reference and returns a complete `CapturedSource` tree. The application supplies those inputs. The target packages them without reading the workspace, starting a process or contacting a server.

For a new input format, start in the [Mintlify assembler](../adapters/mintlify/src/assemble.rs). The [adapter entry point](../adapters/mintlify/src/lib.rs) supplies the Markdown parser and preview link map. Reuse the link and heading handling in [manual pages](../adapters/mintlify/src/manual.rs) and [generated Markdown](../adapters/mintlify/src/markdown.rs). Apply the output limits to the whole tree. Compare the generated declarations, examples and links with the input, then open the assembled site in Mintlify.
