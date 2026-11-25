# Gazette

Gazette publishes a body of thought from the whole to its parts, and makes that structure what readers, search, feeds, JSON-LD and `llms.txt` index. A collection develops one question, a subcollection takes up a branch of it, a journey sets the order an argument needs, and a crossing lets a reader follow the thought into another publication without leaving the argument. Gazette declares and checks that structure, where a static-site generator groups pages without saying what the grouping means. Among the products [RawThoughts](https://rawthoughts.in/) builds, Gazette carries approved work outward, and it works as a product of its own.

Gazette is a publishing compiler written in Rust. It turns authored content and a workspace's publication structure into websites, with HTML, navigation, search, feeds and structured data drawn from the same model. An article's identity, authorship, reading relationships and address are defined once instead of being reconstructed by each output.

Use one workspace for a single publication or several connected ones. Each publication can select its own Theme while sharing content sources and links to other work. The command line, MCP server, language server and SDKs expose the same compiler and application, so an editor, agent and build tool can work with the same publication.

## What it does

- Organizes articles into collections and ordered journeys, with explicit routes, aliases, authorship and project relationships.
- Resolves links within a publication and across configured publications, including local preview addresses. [Links and crossings](docs/links-and-crossings.md) explains what Themes receive.
- Renders static HTML through Tera Themes, with optional components and browser behavior. Writing stays separate from presentation.
- Includes figures, notes, mathematics, diagrams and saved notebook results. Page resources support selected JavaScript/TypeScript, stylesheets, fonts, images and inert SVG through their documented compilation profiles.
- Produces search data, feeds, sitemaps, JSON-LD, Markdown counterparts and `llms.txt` from the publication model.
- Provides compiler feedback in VS Code and a local preview that rebuilds as you save writing and Theme changes.
- Assembles repository manuals and generated library/API reference into Mintlify documentation sites.
- Records publication state and release history in SQLite, and supports reviewed output preparation, authorized Git delivery and destination read-back.

Gazette runs locally from source. It is not a hosted publishing service, and its unreleased APIs and wire formats may change.

## Getting started

To create a publication you can keep writing in, follow [Create your first publication](https://rawthoughts.in/gazette/first-publication/). It takes you from the source-build prerequisite to an editable collection, two articles and a browser preview.

For code development, [build Gazette from source](docs/install.md#what-a-build-needs). The developer guide then leads to the integration you need:

| You want to | Start with |
| --- | --- |
| Embed Gazette in a Node application | [JavaScript SDK example](sdk/js/README.md#check-a-configuration-and-locate-a-field) |
| Understand or extend compilation | [Architecture](docs/architecture.md), [local rendering](docs/local-rendering.md) |
| Implement a Theme or reader tool | [Theme contract](docs/theme-reference.md), [browser SDK](sdk/js/docs/browser.md) |
| Build developer documentation | [Mintlify integration](docs/mintlify.md) |

The [disposable quickstart](examples/quickstart/README.md) checks a checkout by building a temporary one-site workspace. It removes that example when it finishes; it is a contributor check rather than a place to continue authoring.

### Agents and editors

The built executable serves local MCP and LSP over standard input/output:

```sh
gazette mcp
gazette lsp
```

Register the executable with your client rather than running both modes in one terminal. For everyday use, follow the product tasks for [working with an agent](https://rawthoughts.in/gazette/work-with-an-agent/) or [writing in the editor](https://rawthoughts.in/gazette/write-with-feedback/). Developers implementing a client can use the [MCP](docs/mcp.md), [LSP](docs/lsp.md) and [VS Code](docs/vscode.md) contracts.

## Architecture

The compiler parses retained source bytes, resolves publication relationships and routes, then supplies page contexts and machine-readable outputs. Themes render those contexts; they do not infer publication structure from filenames or perform live source lookups.

The application coordinates source reads, rendering, previews, storage and delivery. Ports describe the capabilities it needs, and adapters implement filesystem, Git, SQLite, Tera, processes and documentation conversion. The Rust SDK selects these implementations; the other interfaces expose its operations. The [architecture guide](docs/architecture.md) follows a title edit and a crossing through that flow, explains the tradeoffs and locates the code for extensions.

## Previews

These six live publications share [Base](https://github.com/getHarshOnline/gazette-theme-base) for navigation, reading, search and controls. Each Theme gives its publication its own look. Select a preview to open the site.

|  |  |
| --- | --- |
| [![getHarsh Home](https://raw.githubusercontent.com/getHarshOnline/gazette-theme-chronicle/main/docs/preview.gif)](https://getharsh.in/)<br>[Chronicle theme](https://github.com/getHarshOnline/gazette-theme-chronicle) · [getHarsh site repository](https://github.com/getHarsh/getharsh.in) | [![RawThoughts Home](https://raw.githubusercontent.com/getHarshOnline/gazette-theme-carrier/main/docs/preview.gif)](https://rawthoughts.in/)<br>[Carrier theme](https://github.com/getHarshOnline/gazette-theme-carrier) · [RawThoughts site repository](https://github.com/getHarshOnline/rawThoughts.in) |
| [![Causality Home](https://raw.githubusercontent.com/getHarshOnline/gazette-theme-lattice/main/docs/preview.gif)](https://causality.in/)<br>[Lattice theme](https://github.com/getHarshOnline/gazette-theme-lattice) · [Causality site repository](https://github.com/getHarshOnline/causality.in) | [![Post-AGI Economics Home](https://raw.githubusercontent.com/getHarshOnline/gazette-theme-folio/main/docs/preview.gif)](https://post-agi-economics.org/)<br>[Folio theme](https://github.com/getHarshOnline/gazette-theme-folio) · [Post-AGI Economics site repository](https://github.com/getHarshOnline/post-agi-economics.org) |
| [![Sleepwalker Home](https://raw.githubusercontent.com/getHarshOnline/gazette-theme-nocturne/main/docs/preview.gif)](https://sleepwalker.in/)<br>[Nocturne theme](https://github.com/getHarshOnline/gazette-theme-nocturne) · [Sleepwalker site repository](https://github.com/getHarshOnline/sleepwalker.in) | [![DAO Studio Home](https://raw.githubusercontent.com/getHarshOnline/gazette-theme-chassis/main/docs/preview.gif)](https://daostudio.in/)<br>[Chassis theme](https://github.com/getHarshOnline/gazette-theme-chassis) · [DAO Studio site repository](https://github.com/DAO-Studio/daostudio.in) |

## Publishing

A preview, a generated output tree and a public release are different results. Gazette can retain local output for review, prepare an exact release, verify the publisher's authorization and push to the configured Git destination. GitHub Pages or Mintlify then builds that repository. Destination read-back and served-page checks establish what actually arrived.

An interrupted delivery keeps its recorded outcome for recovery; it is not silently retried. See [releasing](docs/releasing.md), [store](docs/store.md) and [recovery](docs/recovery.md) when integrating that workflow.

## Three documentation paths

There are two documentation audiences, with three kinds of source material:

1. **Product help** teaches understanding and using the product. It is authored Content, composed as a documentation collection alongside the publication.
2. **Developer manuals** explain code integration, architecture, source builds and extension. They stay in the repository and are selected for Mintlify. Contributor checklists stay in the source checkout, reached through [Contributing](CONTRIBUTING.md).
3. **Generated reference** comes from code declarations and comments. Python, Rust, TypeScript, Go and HTTP reference use their selected generator/format profiles and sit beside the developer manuals.

The [documentation guide](docs/mintlify.md) explains assembly and supported input formats; [generator integration](docs/documentation-generators.md) covers running the selected tools. The workspace chooses the project modules and navigation.

## Configuration and facts

A workspace selects content sources, publications, canonical addresses, Themes, resources and output destinations. Content declarations define each contribution's identity and relationships. Capturing the selected inputs before compilation keeps the page and its discovery outputs tied to the same source version.

Gazette currently uses locally declared facts. A provider-neutral port exists for external records, but the Weave integration is not implemented. See [workspace configuration](docs/workspace.md), [content structure](docs/content-model.md) and [routing](docs/routing.md).

## Documentation

The [developer guide](docs/README.md) connects the integration guides and exact references. Product tasks are collected at [Gazette](https://rawthoughts.in/gazette/).

## Development

[Build and test setup](docs/install.md) lists the tool versions, executable paths and checks used by contributors. Tests live with their owning packages; architecture checks enforce the dependency boundaries. [Contributing](CONTRIBUTING.md) explains the development rules and leads to the maintenance chapters for dependencies, generators, MCP and VS Code.

Rust API documentation stays with its items. Public operations describe the inputs, effects, limits and errors a caller needs to handle. Documentation lints check those contracts and their links. Run `python3 tools/generate_distribution_evidence.py --check` to validate those inputs without writing files, or pass `--output-root` to prepare a release or temporary directory. The [distribution guide](docs/install.md#generate-distribution-notices) explains the output and its limits.

Build and test Gazette locally on Linux x86-64 or macOS arm64. Run `python3 tools/check_dependencies.py --help` to scan the locked dependency graphs with the documented tools. Review advisories with the release inputs.

## Licence

Gazette is free software under the GNU Affero General Public License, version 3 or later. See [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE). The browser modules a theme bundles into its pages, [`search`](sdk/js/search), [`analytics`](sdk/js/analytics) and [`webmcp`](sdk/js/webmcp), are licensed under the Apache License, Version 2.0 instead; each carries its own `LICENSE` and `NOTICE`.

Commercial use is permitted under the applicable licence. Alternative terms require a separate written agreement with RawThoughts Enterprises Private Limited; this repository grants none. See [NOTICE](NOTICE) for ownership, [CITATION.cff](CITATION.cff) for citation and [CONTRIBUTING.md](CONTRIBUTING.md) for contribution requirements.
