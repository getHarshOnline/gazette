# Gazette developer guide

Gazette publishes a body of thought from the whole to its parts, and makes that structure what readers, search, feeds, JSON-LD and `llms.txt` index. This guide is for developers who build an editor integration, extend a Theme or embed Gazette in an application: the Rust compiler resolves content, relationships and addresses once for the CLI, agent server, editors and SDKs. Among the products [RawThoughts](https://rawthoughts.in/) builds, Gazette carries approved work outward, and it works as a product of its own.

To write and preview a publication, start with [Your first publication](https://rawthoughts.in/gazette/first-publication/).

## Start with your integration

::::card-group

:::card

[Build Gazette](install.md#what-a-build-needs)

Compile the executable and continue with a publication or integration.

:::

:::card

[Understand the architecture](architecture.md)

Follow a content change from source files through the compiler to its outputs.

:::

:::card

[Connect an MCP client](mcp-quickstart.md)

Start the local server and check a workspace from your client.

:::

:::card

[Use the JavaScript SDK](../sdk/js/README.md#check-a-configuration-and-locate-a-field)

Check a complete configuration from Node and locate a field in its source.

:::

::::

[Publication previews](previews.md) show the accepted designs of six publications made for Gazette, each with its own Theme; none is live yet.

## Extend Gazette

| You are building | Read |
| --- | --- |
| Editor support | [LSP](lsp.md), [VS Code](vscode.md) |
| A theme or component | [Theme contract](theme-reference.md), [Tera](tera.md) |
| Browser search and other browser features | [Browser SDK](../sdk/js/docs/browser.md), [capability bindings](browser-capabilities.md) |
| A developer-documentation build | [Mintlify integration](mintlify.md), [documentation generators](documentation-generators.md) |
| Delivery or recovery tooling | [Release operations](releasing.md), [store](store.md), [recovery](recovery.md) |

## Look up a contract

[Workspace configuration](workspace.md) covers how to validate the setup, read its sources and choose a Theme. The [content model](content-model.md) explains pages, collections, journeys and authorship. Use [Markdown](markdown.md), [page resources](page-resources.md) and [notebooks](notebooks.md) for the supported source formats.

Use [local rendering](local-rendering.md) to build or inspect site files and [preview](preview.md) to serve them locally. See [routing](routing.md), [links and crossings](links-and-crossings.md), [feeds](feeds.md), [JSON-LD](json-ld.md) and [deployment files](deployment-output.md) for what Gazette emits. To update a configured destination, follow [output materialization](output-materialization.md).

Integrations that prepare public output also need [policy checks](policy-checks.md), [disclosure checks](output-disclosure.md) and [delivery observation](delivery.md). These operations are separate from ordinary analysis and rendering.

Use `gazette schema` to print a declaration schema, and `gazette <command> --help` for command inputs and permissions.

Gazette is licensed under the GNU Affero General Public License, version 3 or later. See [LICENSE](../LICENSE) and the [distribution notices](install.md#generate-distribution-notices).
